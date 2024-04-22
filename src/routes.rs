use hiro_system_kit::slog;
use hyper::{Body, Client, Request, Response, Uri};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    convert::Infallible,
    str::FromStr,
    sync::{Arc, Mutex},
};

use crate::{
    config::StacksDevnetConfig,
    resources::service::{get_service_from_path_part, get_service_url, get_user_facing_port},
    responder::Responder,
    Context, StacksDevnetApiK8sManager, StacksDevnetInfoResponse,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PRJ_NAME: &str = env!("CARGO_PKG_NAME");

pub async fn handle_get_status(
    responder: Responder,
    ctx: Context,
) -> Result<Response<Body>, Infallible> {
    let version_info = format!("{PRJ_NAME} v{VERSION}");
    let version_info = json!({ "version": version_info });
    let version_info = match serde_json::to_vec(&version_info) {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("failed to parse version info: {}", e.to_string());
            ctx.try_log(|logger| slog::error!(logger, "{}", msg));
            return responder.err_internal(msg);
        }
    };
    let body = Body::from(version_info);
    responder.ok_with_json(body)
}

pub async fn handle_new_devnet(
    request: Request<Body>,
    user_id: &str,
    k8s_manager: StacksDevnetApiK8sManager,
    responder: Responder,
    request_store: Arc<Mutex<HashMap<String, u64>>>,
    request_time: u64,
    ctx: &Context,
) -> Result<Response<Body>, Infallible> {
    let body = hyper::body::to_bytes(request.into_body()).await;
    if body.is_err() {
        let msg = "failed to parse request body";
        ctx.try_log(|logger| slog::error!(logger, "{}", msg));
        return responder.err_internal(msg.into());
    }
    let body = body.unwrap();
    let config: Result<StacksDevnetConfig, _> = serde_json::from_slice(&body);
    match config {
        Ok(config) => match config.to_validated_config(user_id, ctx) {
            Ok(config) => match k8s_manager.deploy_devnet(config).await {
                Ok(_) => {
                    match request_store.lock() {
                        Ok(mut store) => {
                            store.insert(user_id.to_string(), request_time);
                        }
                        Err(_) => {}
                    }
                    responder.ok()
                }
                Err(e) => responder.respond(e.code, e.message),
            },
            Err(e) => responder.respond(e.code, e.message),
        },
        Err(e) => {
            responder.err_bad_request(format!("invalid configuration to create network: {}", e))
        }
    }
}

pub async fn handle_delete_devnet(
    k8s_manager: StacksDevnetApiK8sManager,
    network: &str,
    user_id: &str,
    responder: Responder,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.delete_devnet(network, user_id).await {
        Ok(_) => responder.ok(),
        Err(e) => {
            let msg = format!("error deleting network {}: {}", &network, e.message);
            responder.respond(e.code, msg)
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DevnetMetadata {
    pub secs_since_last_request: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StacksDevnetInfoWithMetadata {
    #[serde(flatten)]
    pub data: StacksDevnetInfoResponse,
    pub metadata: DevnetMetadata,
}

pub async fn handle_get_devnet(
    k8s_manager: StacksDevnetApiK8sManager,
    network: &str,
    user_id: &str,
    responder: Responder,
    request_store: Arc<Mutex<HashMap<String, u64>>>,
    request_time: u64,
    ctx: Context,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.get_devnet_info(&network, user_id).await {
        Ok(devnet_info) => {
            let last_request_time = match request_store.lock() {
                Ok(mut store) => match store.get(user_id) {
                    Some(last_request_time) => *last_request_time,
                    None => {
                        store.insert(user_id.to_string(), request_time);
                        request_time
                    }
                },
                Err(_) => 0,
            };
            let devnet_info_with_metadata = StacksDevnetInfoWithMetadata {
                data: devnet_info,
                metadata: DevnetMetadata {
                    secs_since_last_request: request_time.saturating_sub(last_request_time),
                },
            };
            match serde_json::to_vec(&devnet_info_with_metadata) {
                Ok(body) => responder.ok_with_json(Body::from(body)),
                Err(e) => {
                    let msg = format!(
                        "failed to form response body: NAMESPACE: {}, ERROR: {}",
                        &network,
                        e.to_string()
                    );
                    ctx.try_log(|logger: &hiro_system_kit::Logger| slog::error!(logger, "{}", msg));
                    responder.err_internal(msg)
                }
            }
        }
        Err(e) => responder.respond(e.code, e.message),
    }
}

pub async fn handle_check_devnet(
    k8s_manager: StacksDevnetApiK8sManager,
    network: &str,
    user_id: &str,
    responder: Responder,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager
        .check_any_devnet_assets_exist(network, user_id)
        .await
    {
        Ok(assets_exist) => match assets_exist {
            true => responder.ok(),
            false => responder.err_not_found("not found".to_string()),
        },
        Err(e) => responder.respond(e.code, e.message),
    }
}

pub async fn handle_try_proxy_service(
    remaining_path: &str,
    subroute: &str,
    network: &str,
    request: Request<Body>,
    k8s_manager: StacksDevnetApiK8sManager,
    responder: Responder,
    ctx: &Context,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.check_all_devnet_assets_exist(&network).await {
        Ok(exists) => match exists {
            true => {
                let service = get_service_from_path_part(subroute);
                match service {
                    Some(service) => {
                        let base_url = get_service_url(&network, service.clone());
                        let port = get_user_facing_port(service).unwrap();
                        let forward_url = format!("{}:{}", base_url, port);
                        let proxy_request =
                            mutate_request_for_proxy(request, &forward_url, &remaining_path);
                        proxy(proxy_request, responder, &ctx).await
                    }
                    None => responder.err_bad_request("invalid request path".into()),
                }
            }
            false => {
                let msg = format!("not all devnet assets exist NAMESPACE: {}", &network);
                ctx.try_log(|logger: &hiro_system_kit::Logger| slog::info!(logger, "{}", msg));
                responder.err_not_found(msg)
            }
        },
        Err(e) => responder.respond(e.code, e.message),
    }
}

pub fn mutate_request_for_proxy(
    mut request: Request<Body>,
    forward_url: &str,
    path_to_forward: &str,
) -> Request<Body> {
    let query = match request.uri().query() {
        Some(query) => format!("?{}", query),
        None => String::new(),
    };

    *request.uri_mut() = {
        let forward_uri = format!("http://{}/{}{}", forward_url, path_to_forward, query);
        Uri::from_str(forward_uri.as_str())
    }
    .unwrap();
    request
}

async fn proxy(
    request: Request<Body>,
    responder: Responder,
    ctx: &Context,
) -> Result<Response<Body>, Infallible> {
    let client = Client::new();

    ctx.try_log(|logger| slog::info!(logger, "forwarding request to {}", request.uri()));
    match client.request(request).await {
        Ok(response) => Ok(response),
        Err(e) => {
            let msg = format!("error proxying request: {}", e.to_string());
            ctx.try_log(|logger| slog::error!(logger, "{}", msg));
            responder.err_internal(msg)
        }
    }
}
#[derive(Default, PartialEq, Debug, Clone)]
pub struct PathParts {
    pub route: String,
    pub network: Option<String>,
    pub subroute: Option<String>,
    pub remainder: Option<String>,
}
pub const API_PATH: &str = "/api/v1/";
pub fn get_standardized_path_parts(path: &str) -> PathParts {
    let path = path.replace(API_PATH, "");
    let path = path.trim_matches('/');
    let parts: Vec<&str> = path.split("/").collect();

    match parts.len() {
        0 => PathParts {
            route: String::new(),
            ..Default::default()
        },
        1 => PathParts {
            route: parts[0].into(),
            ..Default::default()
        },
        2 => PathParts {
            route: parts[0].into(),
            network: Some(parts[1].into()),
            ..Default::default()
        },
        3 => PathParts {
            route: parts[0].into(),
            network: Some(parts[1].into()),
            subroute: Some(parts[2].into()),
            ..Default::default()
        },
        _ => {
            let remainder = parts[3..].join("/");
            PathParts {
                route: parts[0].into(),
                network: Some(parts[1].into()),
                subroute: Some(parts[2].into()),
                remainder: Some(remainder),
            }
        }
    }
}
