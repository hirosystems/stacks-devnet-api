use hiro_system_kit::slog;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server};
use stacks_devnet_api::api_config::ApiConfig;
use stacks_devnet_api::responder::Responder;
use stacks_devnet_api::routes::{
    get_standardized_path_parts, handle_check_devnet, handle_delete_devnet, handle_get_devnet,
    handle_new_devnet, handle_try_proxy_service, API_PATH,
};
use stacks_devnet_api::{Context, StacksDevnetApiK8sManager};
use std::{convert::Infallible, net::SocketAddr};

#[tokio::main]
async fn main() {
    const HOST: &str = "0.0.0.0";
    const PORT: &str = "8477";
    let endpoint: String = HOST.to_owned() + ":" + PORT;
    let addr: SocketAddr = endpoint.parse().expect("Could not parse ip:port.");

    let logger = hiro_system_kit::log::setup_logger();
    let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
    let ctx = Context {
        logger: Some(logger),
        tracer: false,
    };
    let k8s_manager = StacksDevnetApiK8sManager::default(&ctx).await;
    let config_path = if cfg!(debug_assertions) {
        "./Config.toml"
    } else {
        "/etc/config/Config.toml"
    };
    let config = ApiConfig::from_path(config_path);

    let make_svc = make_service_fn(|_| {
        let k8s_manager = k8s_manager.clone();
        let ctx = ctx.clone();
        let config = config.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(req, k8s_manager.clone(), config.clone(), ctx.clone())
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    ctx.try_log(|logger| slog::info!(logger, "Running server on {:?}", addr));

    if let Err(e) = server.await {
        ctx.try_log(|logger| slog::error!(logger, "server error: {}", e));
    }
}

async fn handle_request(
    request: Request<Body>,
    k8s_manager: StacksDevnetApiK8sManager,
    ApiConfig {
        http_response_config,
        auth_config,
    }: ApiConfig,
    ctx: Context,
) -> Result<Response<Body>, Infallible> {
    let uri = request.uri();
    let path = uri.path();
    let method = request.method();
    ctx.try_log(|logger| {
        slog::info!(
            logger,
            "received request with method {} and path {}",
            method,
            path
        )
    });
    let headers = request.headers().clone();
    let responder = Responder::new(http_response_config, headers.clone()).unwrap();

    if method == &Method::OPTIONS {
        return responder.ok();
    }
    let auth_header = auth_config
        .auth_header
        .unwrap_or("x-auth-request-user".to_string());
    let user_id = match headers.get(auth_header) {
        Some(auth_header_value) => match auth_header_value.to_str() {
            Ok(user_id) => {
                let user_id = user_id.replace("|", "-");
                match auth_config.namespace_prefix {
                    Some(mut prefix) => {
                        prefix.push_str(&user_id);
                        prefix
                    }
                    None => user_id,
                }
            }
            Err(e) => {
                let msg = format!("unable to parse auth header: {}", &e);
                ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
                return responder.err_bad_request(msg);
            }
        },
        None => return responder.err_bad_request("missing required auth header".into()),
    };

    if path == "/api/v1/networks" {
        return match method {
            &Method::POST => {
                handle_new_devnet(request, &user_id, k8s_manager, responder, ctx).await
            }
            _ => responder.err_method_not_allowed("network creation must be a POST request".into()),
        };
    } else if path.starts_with(API_PATH) {
        let path_parts = get_standardized_path_parts(uri.path());

        if path_parts.route != "network" {
            return responder.err_bad_request("invalid request path".into());
        }
        // the api path must be followed by a network id
        if path_parts.network.is_none() {
            return responder.err_bad_request("no network id provided".into());
        }
        let network = path_parts.network.unwrap();
        if network != user_id {
            return responder.err_bad_request("network id must match authenticated user id".into());
        }

        // verify that we have a valid namespace and the network actually exists
        let exists = match k8s_manager.check_namespace_exists(&network).await {
            Ok(exists) => exists,
            Err(e) => {
                return responder.respond(e.code, e.message);
            }
        };
        if !exists {
            let msg = format!("network {} does not exist", &network);
            ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
            return responder.err_not_found(msg);
        }

        // the path only contained the network path and network id,
        // so it must be a request to DELETE a network or GET network info
        if path_parts.subroute.is_none() {
            return match method {
                &Method::DELETE => handle_delete_devnet(k8s_manager, &network, responder).await,
                &Method::GET => handle_get_devnet(k8s_manager, &network, responder, ctx).await,
                &Method::HEAD => handle_check_devnet(k8s_manager, &network, responder).await,
                _ => responder
                    .err_method_not_allowed("can only GET/DELETE/HEAD at provided route".into()),
            };
        }
        let subroute = path_parts.subroute.unwrap();
        if subroute == "commands" {
            return responder.err_not_implemented("commands route in progress".into());
        } else {
            let remaining_path = path_parts.remainder.unwrap_or(String::new());
            return handle_try_proxy_service(
                &remaining_path,
                &subroute,
                &network,
                request,
                k8s_manager,
                responder,
                &ctx,
            )
            .await;
        }
    }

    responder.err_bad_request("invalid request path".into())
}

#[cfg(test)]
pub mod tests;
