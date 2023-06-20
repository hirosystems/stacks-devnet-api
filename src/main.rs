use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode, Uri};
use stacks_devnet_api::{StacksDevnetApiK8sManager, StacksDevnetConfig};
use std::net::IpAddr;
use std::str::FromStr;
use std::{convert::Infallible, net::SocketAddr};

#[tokio::main]
async fn main() {
    const HOST: &str = "0.0.0.0";
    const PORT: &str = "8477";
    let endpoint: String = HOST.to_owned() + ":" + PORT;
    let addr: SocketAddr = endpoint.parse().expect("Could not parse ip:port.");
    let k8s_manager = StacksDevnetApiK8sManager::default().await;

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let k8s_manager = k8s_manager.clone();
        let remote_addr = conn.remote_addr().ip();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(remote_addr, req, k8s_manager.clone())
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Running server on {:?}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

fn mutate_request_for_proxy(
    mut request: Request<Body>,
    network: &str,
    path_to_forward: &str,
    proxy_data: ProxyData,
) -> Request<Body> {
    let forward_url = format!(
        "http://{}.{}.svc.cluster.local:{}",
        proxy_data.destination_service, network, proxy_data.destination_port
    );

    let query = match request.uri().query() {
        Some(query) => format!("?{}", query),
        None => String::new(),
    };

    *request.uri_mut() = {
        let forward_uri = format!("{}/{}{}", forward_url, path_to_forward, query);
        Uri::from_str(forward_uri.as_str())
    }
    .unwrap();
    request
}

async fn proxy(request: Request<Body>) -> Result<Response<Body>, Infallible> {
    let client = Client::new();

    println!("forwarding request to {}", request.uri());
    match client.request(request).await {
        Ok(response) => Ok(response),
        Err(_error) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap()),
    }
}

struct ProxyData {
    destination_service: String,
    destination_port: String,
}
fn get_proxy_data(proxy_path: &str) -> Option<ProxyData> {
    const BITCOIN_NODE_PATH: &str = "bitcoin-node";
    const STACKS_NODE_PATH: &str = "stacks-node";
    const STACKS_API_PATH: &str = "stacks-api";
    const BITCOIN_NODE_SERVICE: &str = "bitcoind-chain-coordinator-service";
    const STACKS_NODE_SERVICE: &str = "stacks-node-service";
    const STACKS_API_SERVICE: &str = "stacks-api-service";
    const BITCOIN_NODE_PORT: &str = "18443";
    const STACKS_NODE_PORT: &str = "20443";
    const STACKS_API_PORT: &str = "3999";

    match proxy_path {
        BITCOIN_NODE_PATH => Some(ProxyData {
            destination_service: BITCOIN_NODE_SERVICE.into(),
            destination_port: BITCOIN_NODE_PORT.into(),
        }),
        STACKS_NODE_PATH => Some(ProxyData {
            destination_service: STACKS_NODE_SERVICE.into(),
            destination_port: STACKS_NODE_PORT.into(),
        }),
        STACKS_API_PATH => Some(ProxyData {
            destination_service: STACKS_API_SERVICE.into(),
            destination_port: STACKS_API_PORT.into(),
        }),
        _ => None,
    }
}

const API_PATH: &str = "/api/v1/";
#[derive(Default, PartialEq, Debug)]
struct PathParts {
    route: String,
    network: Option<String>,
    subroute: Option<String>,
    remainder: Option<String>,
}
fn get_standardized_path_parts(path: &str) -> PathParts {
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

async fn handle_request(
    _client_ip: IpAddr,
    request: Request<Body>,
    k8s_manager: StacksDevnetApiK8sManager,
) -> Result<Response<Body>, Infallible> {
    let uri = request.uri();
    let path = uri.path();
    let method = request.method();
    println!("received request, method: {}. path: {}", method, path);

    if path == "/api/v1/networks" {
        return match method {
            &Method::POST => {
                let body = hyper::body::to_bytes(request.into_body()).await;
                if body.is_err() {
                    return Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::try_from("failed to parse request body").unwrap())
                        .unwrap());
                }
                let body = body.unwrap();
                let config: Result<StacksDevnetConfig, _> = serde_json::from_slice(&body);
                if config.is_err() {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::try_from("invalid configuration to create network").unwrap())
                        .unwrap());
                }
                let config = config.unwrap();
                match k8s_manager.deploy_devnet(config).await {
                    Ok(_) => Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::empty())
                        .unwrap()),
                    Err(e) => Ok(Response::builder()
                        .status(StatusCode::from_u16(e.code).unwrap())
                        .body(Body::try_from(e.message).unwrap())
                        .unwrap()),
                }
            }
            _ => Ok(Response::builder()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body(Body::try_from("network creation must be a POST request").unwrap())
                .unwrap()),
        };
    } else if path.starts_with(API_PATH) {
        let path_parts = get_standardized_path_parts(uri.path());

        if path_parts.route != "network" {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::try_from("invalid request path").unwrap())
                .unwrap());
        }
        // the api path must be followed by a network id
        if path_parts.network.is_none() {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::try_from("no network id provided").unwrap())
                .unwrap());
        }
        let network = path_parts.network.unwrap();

        // verify that we have a valid namespace and the network actually exists
        let exists = match k8s_manager.check_namespace_exists(&network).await {
            Ok(exists) => exists,
            Err(e) => {
                return Ok(Response::builder()
                    .status(StatusCode::from_u16(e.code).unwrap())
                    .body(Body::try_from(e.message).unwrap())
                    .unwrap());
            }
        };
        if !exists {
            return Ok(Response::builder()
                .status(StatusCode::from_u16(404).unwrap())
                .body(Body::try_from("network does not exist").unwrap())
                .unwrap());
        }

        // the path only contained the network path and network id,
        // so it must be a request to DELETE a network or GET network info
        if path_parts.subroute.is_none() {
            return match method {
                &Method::DELETE => match k8s_manager.delete_devnet(&network).await {
                    Ok(_) => Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::empty())
                        .unwrap()),
                    Err(_e) => Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                        .unwrap()),
                },
                &Method::GET => Ok(Response::builder()
                    .status(StatusCode::NOT_IMPLEMENTED)
                    .body(Body::empty())
                    .unwrap()),
                _ => Ok(Response::builder()
                    .status(StatusCode::METHOD_NOT_ALLOWED)
                    .body(Body::empty())
                    .unwrap()),
            };
        }
        let subroute = path_parts.subroute.unwrap();
        if subroute == "commands" {
            return Ok(Response::builder()
                .status(StatusCode::NOT_IMPLEMENTED)
                .body(Body::empty())
                .unwrap());
        } else {
            let remaining_path = path_parts.remainder.unwrap_or(String::new());

            let proxy_data = get_proxy_data(&subroute);
            return match proxy_data {
                Some(proxy_data) => {
                    let proxy_request =
                        mutate_request_for_proxy(request, &network, &remaining_path, proxy_data);
                    proxy(proxy_request).await
                }
                None => Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::try_from("invalid request path").unwrap())
                    .unwrap()),
            };
        }
    }

    Ok(Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::try_from("invalid request path").unwrap())
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::body;
    use k8s_openapi::api::core::v1::Namespace;
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

        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default").await;
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
            let mut response = handle_request(client_ip, request, k8s_manager.clone())
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

        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default").await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/network/undeployed";

        let request_builder = Request::builder().uri(path).method("GET");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let mut response = handle_request(client_ip, request, k8s_manager.clone())
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

        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default").await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/network/";

        let request_builder = Request::builder().uri(path).method("GET");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let mut response = handle_request(client_ip, request, k8s_manager.clone())
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

        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default").await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/networks";

        let methods = ["GET", "DELETE"];
        for method in methods {
            let request_builder = Request::builder().uri(path).method(method);
            let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
            let mut response = handle_request(client_ip, request, k8s_manager.clone())
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

        let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default").await;
        let client_ip: IpAddr = IpAddr::V4([0, 0, 0, 0].into());
        let path = "/api/v1/networks";

        let request_builder = Request::builder().uri(path).method("POST");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let mut response = handle_request(client_ip, request, k8s_manager.clone())
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
        println!("{}", &remainder);
        let proxy_data = get_proxy_data(&subroute);
        let request_builder = Request::builder().uri("/").method("POST");
        let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
        let request = mutate_request_for_proxy(request, &network, &remainder, proxy_data.unwrap());
        let actual_url = request.uri().to_string();
        let expected = format!(
            "http://stacks-node-service.{}.svc.cluster.local:20443/{}",
            network, &remainder
        );
        println!("{expected}");
        assert_eq!(actual_url, expected);
    }
}
