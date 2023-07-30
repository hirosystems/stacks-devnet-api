use hiro_system_kit::slog;
use hyper::{Body, Client, Request, Response, StatusCode, Uri};
use std::{convert::Infallible, str::FromStr};

use crate::{
    config::StacksDevnetConfig,
    resources::service::{get_service_from_path_part, get_service_url, get_user_facing_port},
    responder::Responder,
    Context, StacksDevnetApiK8sManager,
};

pub async fn handle_new_devnet(
    request: Request<Body>,
    k8s_manager: StacksDevnetApiK8sManager,
    responder: Responder,
    ctx: Context,
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
        Ok(config) => match config.to_validated_config(ctx) {
            Ok(config) => match k8s_manager.deploy_devnet(config).await {
                Ok(_) => responder.ok(),
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
    responder: Responder,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.delete_devnet(network).await {
        Ok(_) => responder.ok(),
        Err(e) => {
            let msg = format!("error deleting network {}: {}", &network, e.to_string());
            responder.err_internal(msg)
        }
    }
}

pub async fn handle_get_devnet(
    k8s_manager: StacksDevnetApiK8sManager,
    network: &str,
    responder: Responder,
    ctx: Context,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.get_devnet_info(&network).await {
        Ok(devnet_info) => match serde_json::to_vec(&devnet_info) {
            Ok(body) => Ok(responder
                .response_builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/json")
                .body(Body::from(body))
                .unwrap()),
            Err(e) => {
                let msg = format!(
                    "failed to form response body: NAMESPACE: {}, ERROR: {}",
                    &network,
                    e.to_string()
                );
                ctx.try_log(|logger: &hiro_system_kit::Logger| slog::error!(logger, "{}", msg));
                responder.err_internal(msg)
            }
        },
        Err(e) => responder.respond(e.code, e.message),
    }
}

pub async fn handle_try_proxy_service(
    remaining_path: &str,
    subroute: &str,
    network: &str,
    request: Request<Body>,
    responder: Responder,
    ctx: &Context,
) -> Result<Response<Body>, Infallible> {
    let service = get_service_from_path_part(subroute);
    return match service {
        Some(service) => {
            let base_url = get_service_url(&network, service.clone());
            let port = get_user_facing_port(service).unwrap();
            let forward_url = format!("{}:{}", base_url, port);
            let proxy_request = mutate_request_for_proxy(request, &forward_url, &remaining_path);
            proxy(proxy_request, responder, &ctx).await
        }
        None => responder.err_bad_request("invalid request path".into()),
    };
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
