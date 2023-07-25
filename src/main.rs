use hiro_system_kit::slog;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server};
use stacks_devnet_api::responder::Responder;
use stacks_devnet_api::routes::{
    get_standardized_path_parts, handle_delete_devnet, handle_get_devnet, handle_new_devnet,
    handle_try_proxy_service, API_PATH,
};
use stacks_devnet_api::{Context, StacksDevnetApiK8sManager};
use std::net::IpAddr;
use std::{convert::Infallible, net::SocketAddr};

#[tokio::main]
async fn main() {
    const HOST: &str = "0.0.0.0";
    const PORT: &str = "8478";
    let endpoint: String = HOST.to_owned() + ":" + PORT;
    let addr: SocketAddr = endpoint.parse().expect("Could not parse ip:port.");

    let logger = hiro_system_kit::log::setup_logger();
    let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
    let ctx = Context {
        logger: Some(logger),
        tracer: false,
    };
    let k8s_manager = StacksDevnetApiK8sManager::default(&ctx).await;

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let k8s_manager = k8s_manager.clone();
        let ctx = ctx.clone();
        let remote_addr = conn.remote_addr().ip();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(remote_addr, req, k8s_manager.clone(), ctx.clone())
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
    _client_ip: IpAddr,
    request: Request<Body>,
    k8s_manager: StacksDevnetApiK8sManager,
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

    let responder = Responder {
        headers: request.headers().clone(),
        ..Default::default()
    };

    if path == "/api/v1/networks" {
        return match method {
            &Method::POST => handle_new_devnet(request, k8s_manager, responder, ctx).await,
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
                _ => {
                    responder.err_method_not_allowed("can only GET/DELETE at provided route".into())
                }
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
                responder,
                &ctx,
            )
            .await;
        }
    }

    responder.err_bad_request("invalid request path".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::{body, StatusCode};
    use k8s_openapi::api::core::v1::Namespace;
    use stacks_devnet_api::{
        resources::service::{
            get_service_from_path_part, get_service_port, get_service_url, ServicePort,
            StacksDevnetService,
        },
        routes::{get_standardized_path_parts, mutate_request_for_proxy, PathParts},
    };
    use tower_test::mock::{self, Handle};

    async fn mock_k8s_handler(handle: &mut Handle<Request<Body>, Response<Body>>) {
        let (request, send) = handle.next_request().await.expect("Service not called");

        let (body, status) = match (
            request.method().as_str(),
            request.uri().to_string().as_str(),
        ) {
            ("GET", "/api/v1/namespaces/test") => {
                let pod: Namespace = serde_json::from_value(serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "Namespace",
                    "metadata": {
                        "name": "test",
                        "labels": {
                            "name": "test"
                        }
                    },
                }))
                .unwrap();
                (serde_json::to_vec(&pod).unwrap(), 200)
            }
            ("GET", "/api/v1/namespaces/undeployed") => (vec![], 404),
            _ => panic!("Unexpected API request {:?}", request),
        };

        send.send_response(
            Response::builder()
                .status(status)
                .body(Body::from(body))
                .unwrap(),
        );
    }

    #[tokio::test]
    async fn it_responds_400_for_invalid_paths() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let _spawned = tokio::spawn(async move {
            mock_k8s_handler(&mut handle).await;
        });

        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: Some(logger),
            tracer: false,
        };
        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default", &ctx).await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let invalid_paths = vec![
            "/path",
            "/api",
            "/api/v1",
            "/api/v1/network2",
            "/api/v1/network/test/invalid_path",
        ];
        for path in invalid_paths {
            let request_builder = Request::builder().uri(path).method("GET");
            let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
            let mut response = handle_request(client_ip, request, k8s_manager.clone(), ctx.clone())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = response.body_mut();
            let bytes = body::to_bytes(body).await.unwrap().to_vec();
            let body_str = String::from_utf8(bytes).unwrap();
            assert_eq!(body_str, "invalid request path");
        }
    }

    #[tokio::test]
    async fn it_responds_404_undeployed_namespaces() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let _spawned = tokio::spawn(async move {
            mock_k8s_handler(&mut handle).await;
        });

        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: Some(logger),
            tracer: false,
        };
        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default", &ctx).await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/network/undeployed";

        let request_builder = Request::builder().uri(path).method("GET");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let mut response = handle_request(client_ip, request, k8s_manager.clone(), ctx)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response.body_mut();
        let bytes = body::to_bytes(body).await.unwrap().to_vec();
        let body_str = String::from_utf8(bytes).unwrap();
        assert_eq!(body_str, "network does not exist");
    }

    #[tokio::test]
    async fn it_responds_400_missing_network() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let _spawned = tokio::spawn(async move {
            mock_k8s_handler(&mut handle).await;
        });

        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: Some(logger),
            tracer: false,
        };
        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default", &ctx).await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/network/";

        let request_builder = Request::builder().uri(path).method("GET");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let mut response = handle_request(client_ip, request, k8s_manager.clone(), ctx)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.body_mut();
        let bytes = body::to_bytes(body).await.unwrap().to_vec();
        let body_str = String::from_utf8(bytes).unwrap();
        assert_eq!(body_str, "no network id provided");
    }

    #[tokio::test]
    async fn network_creation_responds_405_for_non_post_requests() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let _spawned = tokio::spawn(async move {
            mock_k8s_handler(&mut handle).await;
        });

        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: Some(logger),
            tracer: false,
        };
        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default", &ctx).await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/networks";

        let methods = ["GET", "DELETE"];
        for method in methods {
            let request_builder = Request::builder().uri(path).method(method);
            let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
            let mut response = handle_request(client_ip, request, k8s_manager.clone(), ctx.clone())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
            let body = response.body_mut();
            let bytes = body::to_bytes(body).await.unwrap().to_vec();
            let body_str = String::from_utf8(bytes).unwrap();
            assert_eq!(body_str, "network creation must be a POST request");
        }
    }
    #[tokio::test]
    async fn network_creation_responds_400_for_invalid_config_data() {
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let _spawned = tokio::spawn(async move {
            mock_k8s_handler(&mut handle).await;
        });

        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: Some(logger),
            tracer: false,
        };
        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default", &ctx).await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/networks";

        let request_builder = Request::builder().uri(path).method("POST");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let mut response = handle_request(client_ip, request, k8s_manager.clone(), ctx)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.body_mut();
        let bytes = body::to_bytes(body).await.unwrap().to_vec();
        let body_str = String::from_utf8(bytes).unwrap();
        assert_eq!(body_str, "invalid configuration to create network");
    }

    #[test]
    fn request_paths_are_parsed_correctly() {
        let path = "/api/v1/";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::new(),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/some-network";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            network: Some(String::from("some-network")),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/some-network/";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            network: Some(String::from("some-network")),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/some-network/some-subroute";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            network: Some(String::from("some-network")),
            subroute: Some(String::from("some-subroute")),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/some-network/some-subroute/";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            network: Some(String::from("some-network")),
            subroute: Some(String::from("some-subroute")),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/some-network/some-subroute/the/remaining/path";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            network: Some(String::from("some-network")),
            subroute: Some(String::from("some-subroute")),
            remainder: Some(String::from("the/remaining/path")),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/some-network/some-subroute/the/remaining/path/";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            network: Some(String::from("some-network")),
            subroute: Some(String::from("some-subroute")),
            remainder: Some(String::from("the/remaining/path")),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);

        let path = "/api/v1/some-route/some-network/some-subroute/the//remaining//path/";
        let path_parts = get_standardized_path_parts(path);
        let expected = PathParts {
            route: String::from("some-route"),
            network: Some(String::from("some-network")),
            subroute: Some(String::from("some-subroute")),
            remainder: Some(String::from("the//remaining//path")),
            ..Default::default()
        };
        assert_eq!(path_parts, expected);
    }

    #[tokio::test]
    async fn request_mutation_should_create_valid_proxy_destination() {
        let path = "/api/v1/some-route/some-network/stacks-node/the//remaining///path";
        let path_parts = get_standardized_path_parts(path);
        let network = path_parts.network.unwrap();
        let subroute = path_parts.subroute.unwrap();
        let remainder = path_parts.remainder.unwrap();

        let service = get_service_from_path_part(&subroute).unwrap();
        let forward_url = format!(
            "{}:{}",
            get_service_url(&network, service.clone()),
            get_service_port(service, ServicePort::RPC).unwrap()
        );
        let request_builder = Request::builder().uri("/").method("POST");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let request = mutate_request_for_proxy(request, &forward_url, &remainder);
        let actual_url = request.uri().to_string();
        let expected = format!(
            "http://{}.{}.svc.cluster.local:{}/{}",
            StacksDevnetService::StacksNode,
            network,
            get_service_port(StacksDevnetService::StacksNode, ServicePort::RPC).unwrap(),
            &remainder
        );
        assert_eq!(actual_url, expected);
    }
}
