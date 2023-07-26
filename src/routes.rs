use hiro_system_kit::slog;
use hyper::{Body, Client, Request, Response, StatusCode, Uri};
use std::{convert::Infallible, str::FromStr};

use crate::{
    config::StacksDevnetConfig,
    resources::service::{get_service_from_path_part, get_service_url, get_user_facing_port},
    Context, StacksDevnetApiK8sManager,
};

pub async fn handle_new_devnet(
    request: Request<Body>,
    k8s_manager: StacksDevnetApiK8sManager,
    ctx: Context,
) -> Result<Response<Body>, Infallible> {
    let body = hyper::body::to_bytes(request.into_body()).await;
    if body.is_err() {
        let msg = "failed to parse request body";
        ctx.try_log(|logger| slog::error!(logger, "{}", msg));
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::try_from(msg).unwrap())
            .unwrap());
    }
    let body = body.unwrap();
    let config: Result<StacksDevnetConfig, _> = serde_json::from_slice(&body);
    match config {
        Ok(config) => match config.to_validated_config(ctx) {
            Ok(config) => match k8s_manager.deploy_devnet(config).await {
                Ok(_) => Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap()),
                Err(e) => Ok(Response::builder()
                    .status(StatusCode::from_u16(e.code).unwrap())
                    .body(Body::try_from(e.message).unwrap())
                    .unwrap()),
            },
            Err(e) => Ok(Response::builder()
                .status(StatusCode::from_u16(e.code).unwrap())
                .body(Body::try_from(e.message).unwrap())
                .unwrap()),
        },
        Err(e) => Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(
                Body::try_from(format!("invalid configuration to create network: {}", e)).unwrap(),
            )
            .unwrap()),
    }
}

pub async fn handle_delete_devnet(
    k8s_manager: StacksDevnetApiK8sManager,
    network: &str,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.delete_devnet(network).await {
        Ok(_) => Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap()),
        Err(e) => Ok(Response::builder()
            .status(e.code)
            .body(
                Body::try_from(format!(
                    "error deleting network {}: {}",
                    &network, e.message
                ))
                .unwrap(),
            )
            .unwrap()),
    }
}

pub async fn handle_get_devnet(
    k8s_manager: StacksDevnetApiK8sManager,
    network: &str,
    ctx: Context,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.get_devnet_info(&network).await {
        Ok(devnet_info) => match serde_json::to_vec(&devnet_info) {
            Ok(body) => Ok(Response::builder()
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
                Ok(Response::builder()
                    .status(StatusCode::from_u16(500).unwrap())
                    .body(Body::try_from(msg).unwrap())
                    .unwrap())
            }
        },
        Err(e) => Ok(Response::builder()
            .status(StatusCode::from_u16(e.code).unwrap())
            .body(Body::try_from(e.message).unwrap())
            .unwrap()),
    }
}

pub async fn handle_check_devnet(
    k8s_manager: StacksDevnetApiK8sManager,
    network: &str,
) -> Result<Response<Body>, Infallible> {
    match k8s_manager.check_any_devnet_assets_exist(&network).await {
        Ok(assets_exist) => match assets_exist {
            true => Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Body::empty())
                .unwrap()),
            false => Ok(Response::builder()
                .status(StatusCode::from_u16(404).unwrap())
                .body(Body::empty())
                .unwrap()),
        },
        Err(e) => Ok(Response::builder()
            .status(StatusCode::from_u16(e.code).unwrap())
            .body(Body::try_from(e.message).unwrap())
            .unwrap()),
    }
}

pub async fn handle_try_proxy_service(
    remaining_path: &str,
    subroute: &str,
    network: &str,
    request: Request<Body>,
    k8s_manager: StacksDevnetApiK8sManager,
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
                        proxy(proxy_request, &ctx).await
                    }
                    None => Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::try_from("invalid request path").unwrap())
                        .unwrap()),
                }
            }
            false => {
                let msg = format!("not all devnet assets exist NAMESPACE: {}", &network);
                ctx.try_log(|logger: &hiro_system_kit::Logger| slog::info!(logger, "{}", msg));
                Ok(Response::builder()
                    .status(404)
                    .body(Body::try_from(msg).unwrap())
                    .unwrap())
            }
        },
        Err(e) => Ok(Response::builder()
            .status(StatusCode::from_u16(e.code).unwrap())
            .body(Body::try_from(e.message).unwrap())
            .unwrap()),
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

async fn proxy(request: Request<Body>, ctx: &Context) -> Result<Response<Body>, Infallible> {
    let client = Client::new();

    ctx.try_log(|logger| slog::info!(logger, "forwarding request to {}", request.uri()));
    match client.request(request).await {
        Ok(response) => Ok(response),
        Err(e) => {
            let msg = format!("error proxying request: {}", e.to_string());
            ctx.try_log(|logger| slog::error!(logger, "{}", msg));
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::try_from(msg).unwrap())
                .unwrap())
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
