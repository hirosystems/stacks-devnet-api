use std::{
    fs::File,
    io::{BufReader, Read},
    thread::sleep,
    time::Duration,
};

use super::*;
use hyper::{
    body,
    header::{ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN},
    http::HeaderValue,
    Client, HeaderMap, Method, StatusCode,
};
use k8s_openapi::api::core::v1::Namespace;
use stacks_devnet_api::{
    config::StacksDevnetConfig,
    resources::service::{
        get_service_from_path_part, get_service_port, get_service_url, ServicePort,
        StacksDevnetService,
    },
    routes::{get_standardized_path_parts, mutate_request_for_proxy, PathParts},
    StacksDevnetInfoResponse,
};
use test_case::test_case;
use tower_test::mock::{self, Handle};

fn get_template_config() -> StacksDevnetConfig {
    let file_path = "src/tests/fixtures/stacks-devnet-config.json";
    let file = File::open(file_path)
        .unwrap_or_else(|e| panic!("unable to read file {}\n{:?}", file_path, e));
    let mut file_reader = BufReader::new(file);
    let mut file_buffer = vec![];
    file_reader
        .read_to_end(&mut file_buffer)
        .unwrap_or_else(|e| panic!("unable to read file {}\n{:?}", file_path, e));

    let config_file: StacksDevnetConfig = match serde_json::from_slice(&file_buffer) {
        Ok(s) => s,
        Err(e) => {
            panic!("Config file malformatted {}", e.to_string());
        }
    };
    config_file
}

async fn get_k8s_manager() -> (StacksDevnetApiK8sManager, Context) {
    let logger = hiro_system_kit::log::setup_logger();
    let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
    let ctx = Context::empty();
    let k8s_manager = StacksDevnetApiK8sManager::default(&ctx).await;
    (k8s_manager, ctx)
}

fn get_random_namespace() -> String {
    let mut rng = rand::thread_rng();
    let random_digit: u64 = rand::Rng::gen(&mut rng);
    format!("test-ns-{random_digit}")
}

fn assert_not_all_assets_exist_err((code, body): (StatusCode, String)) {
    assert_eq!(code, StatusCode::NOT_FOUND);
    assert!(body.starts_with("not all devnet assets exist NAMESPACE: test-ns-"));
}

fn assert_cannot_delete_devnet_err((code, body): (StatusCode, String)) {
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(body.starts_with("error deleting network test-ns-"));
    assert!(body.contains("cannot delete devnet because assets do not exist NAMESPACE: test-ns-"));
}

fn assert_cannot_delete_devnet_multiple_errs((code, body): (StatusCode, String)) {
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.starts_with("multiple errors occurred while deleting devnet:"));
}

fn assert_cannot_create_devnet_err((code, body): (StatusCode, String)) {
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(
        body.starts_with("cannot create devnet because assets already exist NAMESPACE: test-ns-")
    );
}

fn assert_failed_proxy((code, body): (StatusCode, String)) {
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.starts_with("error proxying request:"),);
}

fn assert_get_network((code, body): (StatusCode, String)) {
    assert_eq!(code, StatusCode::OK);
    let body: StacksDevnetInfoResponse = serde_json::from_str(&body).unwrap();
    assert!(body.bitcoind_node_status.is_some());
    assert!(body.stacks_node_status.is_some());
    assert!(body.stacks_api_status.is_some());
    assert!(body.bitcoind_node_started_at.is_some());
    assert!(body.stacks_node_started_at.is_some());
}

enum TestBody {
    CreateNetwork,
}

#[test_case("/api/v1/network/{namespace}", Method::DELETE, None, false => is equal_to (StatusCode::OK, "Ok".to_string()); "200 for network DELETE request")]
#[test_case("/api/v1/network/{namespace}", Method::DELETE, None, true => using assert_cannot_delete_devnet_multiple_errs; "500 for network DELETE request with multiple errors")]
#[test_case("/api/v1/networks", Method::POST, Some(TestBody::CreateNetwork), true => using assert_cannot_create_devnet_err; "409 for create network POST request if devnet exists")]
#[test_case("/api/v1/network/{namespace}", Method::GET, None, true => using assert_get_network; "200 for network GET request to existing network")]
#[test_case("/api/v1/network/{namespace}", Method::HEAD, None, true => is equal_to (StatusCode::OK, "Ok".to_string()); "200 for network HEAD request to existing network")]
#[test_case("/api/v1/network/{namespace}/stacks-node/v2/info/", Method::GET, None, true => using assert_failed_proxy; "proxies requests to downstream nodes")]
#[serial_test::serial]
#[tokio::test]
async fn it_responds_to_valid_requests_with_deploy(
    mut request_path: &str,
    method: Method,
    body: Option<TestBody>,
    tear_down: bool,
) -> (StatusCode, String) {
    let namespace = &get_random_namespace();

    let new_path: String;
    if request_path.contains("{namespace}") {
        new_path = request_path.replace("{namespace}", &namespace);
        request_path = &new_path;
    }

    let (k8s_manager, ctx) = get_k8s_manager().await;

    let request_builder = Request::builder().uri(request_path).method(method);

    let _ = k8s_manager.deploy_namespace(&namespace).await.unwrap();

    let mut config = get_template_config();
    config.namespace = namespace.to_owned();
    let validated_config = config.to_validated_config(ctx.clone()).unwrap();
    let _ = k8s_manager.deploy_devnet(validated_config).await.unwrap();
    // short delay to allow assets to start
    sleep(Duration::new(5, 0));

    let body = match body {
        None => Body::empty(),
        Some(TestBody::CreateNetwork) => {
            let mut config = get_template_config();
            config.namespace = namespace.to_owned();
            Body::from(serde_json::to_string(&config).unwrap())
        }
    };

    let request: Request<Body> = request_builder.body(body).unwrap();
    let mut response = handle_request(
        request,
        k8s_manager.clone(),
        ResponderConfig::default(),
        ctx,
    )
    .await
    .unwrap();

    let body = response.body_mut();
    let bytes = body::to_bytes(body).await.unwrap().to_vec();
    let mut body_str = String::from_utf8(bytes).unwrap();
    let mut status = response.status();

    if tear_down {
        match k8s_manager.delete_devnet(namespace).await {
            Ok(_) => {}
            Err(e) => {
                body_str = e.message;
                status = StatusCode::from_u16(e.code).unwrap();
            }
        }
    }
    let _ = k8s_manager.delete_namespace(&namespace).await.unwrap();
    (status, body_str)
}

#[test_case("any", Method::OPTIONS, false => is equal_to (StatusCode::OK, "Ok".to_string()); "200 for any OPTIONS request")]
#[test_case("/api/v1/network/{namespace}", Method::DELETE, true => using assert_cannot_delete_devnet_err; "409 for network DELETE request to non-existing network")]
#[test_case("/api/v1/network/{namespace}", Method::GET, true => using assert_not_all_assets_exist_err; "404 for network GET request to non-existing network")]
#[test_case("/api/v1/network/{namespace}", Method::HEAD, true => is equal_to (StatusCode::NOT_FOUND, "not found".to_string()); "404 for network HEAD request to non-existing network")]
#[test_case("/api/v1/network/{namespace}/stacks-node/v2/info/", Method::GET, true => using assert_not_all_assets_exist_err; "404 for proxy requests to downstream nodes of non-existing network")]
#[tokio::test]
async fn it_responds_to_valid_requests(
    mut request_path: &str,
    method: Method,
    set_up: bool,
) -> (StatusCode, String) {
    let namespace = &get_random_namespace();

    let new_path: String;
    if request_path.contains("{namespace}") {
        new_path = request_path.replace("{namespace}", &namespace);
        request_path = &new_path;
    }

    let (k8s_manager, ctx) = get_k8s_manager().await;

    let request_builder = Request::builder().uri(request_path).method(method);

    if set_up {
        let _ = k8s_manager.deploy_namespace(&namespace).await.unwrap();
    }

    let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
    let mut response = handle_request(
        request,
        k8s_manager.clone(),
        ResponderConfig::default(),
        ctx,
    )
    .await
    .unwrap();

    let body = response.body_mut();
    let bytes = body::to_bytes(body).await.unwrap().to_vec();
    let body_str = String::from_utf8(bytes).unwrap();

    if set_up {
        let _ = k8s_manager.delete_namespace(&namespace).await.unwrap();
    }

    (response.status(), body_str)
}

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
        ("GET", "/api/v1/namespaces/500_err") => (vec![], 500),
        _ => panic!("Unexpected API request {:?}", request),
    };

    send.send_response(
        Response::builder()
            .status(status)
            .body(Body::from(body))
            .unwrap(),
    );
}

async fn get_mock_k8s_manager() -> (StacksDevnetApiK8sManager, Context) {
    let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
    let _spawned = tokio::spawn(async move {
        mock_k8s_handler(&mut handle).await;
    });

    let logger = hiro_system_kit::log::setup_logger();
    let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
    let ctx = Context::empty();
    let k8s_manager = StacksDevnetApiK8sManager::new(mock_service, "default", &ctx).await;
    (k8s_manager, ctx)
}

#[test_case("/path", Method::GET => is equal_to (StatusCode::BAD_REQUEST, "invalid request path".to_string()) ; "400 for invalid requet path /path")]
#[test_case("/api", Method::GET => is equal_to (StatusCode::BAD_REQUEST, "invalid request path".to_string()) ; "400 for invalid requet path /api")]
#[test_case("/api/v1", Method::GET => is equal_to (StatusCode::BAD_REQUEST, "invalid request path".to_string()) ; "400 for invalid requet path /api/v1")]
#[test_case("/api/v1/network2", Method::GET => is equal_to (StatusCode::BAD_REQUEST, "invalid request path".to_string()) ; "400 for invalid requet path /api/v1/network2")]
#[test_case("/api/v1/network/undeployed", Method::GET => 
        is equal_to (StatusCode::NOT_FOUND, "network undeployed does not exist".to_string()); "404 for undeployed namespace")]
#[test_case("/api/v1/network/500_err", Method::GET => 
    is equal_to (StatusCode::INTERNAL_SERVER_ERROR, "error getting namespace 500_err: \"\"".to_string()); "forwarded error if fetching namespace returns error")]
#[test_case("/api/v1/network/test", Method::POST => 
    is equal_to (StatusCode::METHOD_NOT_ALLOWED, "can only GET/DELETE/HEAD at provided route".to_string()); "405 for network route with POST request")]
#[test_case("/api/v1/network/test/commands", Method::GET => 
is equal_to (StatusCode::NOT_FOUND, "commands route in progress".to_string()); "404 for network commands route")]
#[test_case("/api/v1/network/", Method::GET => 
        is equal_to (StatusCode::BAD_REQUEST, "no network id provided".to_string()); "400 for missing namespace")]
#[test_case("/api/v1/networks", Method::GET => 
        is equal_to (StatusCode::METHOD_NOT_ALLOWED, "network creation must be a POST request".to_string()); "405 for network creation request with GET method")]
#[test_case("/api/v1/networks", Method::DELETE => 
        is equal_to (StatusCode::METHOD_NOT_ALLOWED, "network creation must be a POST request".to_string()); "405 for network creation request with DELETE method")]
#[test_case("/api/v1/networks", Method::POST => 
        is equal_to (StatusCode::BAD_REQUEST, "invalid configuration to create network: EOF while parsing a value at line 1 column 0".to_string()); "400 for network creation request invalid config")]
#[tokio::test]
async fn it_responds_to_invalid_requests(
    request_path: &str,
    method: Method,
) -> (StatusCode, String) {
    let (k8s_manager, ctx) = get_mock_k8s_manager().await;

    let request_builder = Request::builder().uri(request_path).method(method);
    let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
    let mut response = handle_request(
        request,
        k8s_manager.clone(),
        ResponderConfig::default(),
        ctx,
    )
    .await
    .unwrap();
    let body = response.body_mut();
    let bytes = body::to_bytes(body).await.unwrap().to_vec();
    let body_str = String::from_utf8(bytes).unwrap();
    (response.status(), body_str)
}

#[test_case("" => is equal_to PathParts { route: String::new(), ..Default::default() }; "for empty path")]
#[test_case("/api/v1/" => is equal_to PathParts { route: String::new(), ..Default::default() }; "for /api/v1/ path")]
#[test_case("/api/v1/some-route" => is equal_to PathParts { route: String::from("some-route"), ..Default::default() }; "for /api/v1/some-route path")]
#[test_case("/api/v1/some-route/" => is equal_to PathParts { route: String::from("some-route"), ..Default::default() }; "for /api/v1/some-route/ path trailing slash")]
#[test_case("/api/v1/some-route/some-network" => is equal_to PathParts { route: String::from("some-route"), network: Some(String::from("some-network")), ..Default::default() }; "for /api/v1/some-route/some-network path")]
#[test_case("/api/v1/some-route/some-network/" => is equal_to PathParts { route: String::from("some-route"), network: Some(String::from("some-network")), ..Default::default() }; "for /api/v1/some-route/some-network/ path trailing slash")]
#[test_case("/api/v1/some-route/some-network/some-subroute" => is equal_to PathParts { route: String::from("some-route"), network: Some(String::from("some-network")), subroute: Some(String::from("some-subroute")), ..Default::default() }; "for /api/v1/some-route/some-network/some-subroute path")]
#[test_case("/api/v1/some-route/some-network/some-subroute/" => is equal_to PathParts { route: String::from("some-route"), network: Some(String::from("some-network")), subroute: Some(String::from("some-subroute")), ..Default::default() }; "for /api/v1/some-route/some-network/some-subroute/ path trailing slash")]
#[test_case("/api/v1/some-route/some-network/some-subroute/the/remaining/path" => is equal_to PathParts { route: String::from("some-route"), network: Some(String::from("some-network")), subroute: Some(String::from("some-subroute")), remainder: Some(String::from("the/remaining/path")), ..Default::default() }; "for /api/v1/some-route/some-network/some-subroute/the/remaining/path path ")]
#[test_case("/api/v1/some-route/some-network/some-subroute/the/remaining/path/" => is equal_to PathParts { route: String::from("some-route"), network: Some(String::from("some-network")), subroute: Some(String::from("some-subroute")), remainder: Some(String::from("the/remaining/path")), ..Default::default() }; "for /api/v1/some-route/some-network/some-subroute/the/remaining/path/ path trailing slash")]
#[test_case("/api/v1/some-route/some-network/some-subroute/the//remaining//path/" => is equal_to PathParts { route: String::from("some-route"), network: Some(String::from("some-network")), subroute: Some(String::from("some-subroute")), remainder: Some(String::from("the//remaining//path")), ..Default::default() }; "for /api/v1/some-route/some-network/some-subroute/the//remaining//path/ path extra internal slash")]
fn request_paths_are_parsed_correctly(path: &str) -> PathParts {
    get_standardized_path_parts(path)
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

#[test]
fn responder_allows_configuring_allowed_origins() {
    let config = ResponderConfig {
        allowed_origins: Some(vec!["*".to_string()]),
        allowed_methods: Some(vec!["GET".to_string()]),
    };
    let mut headers = HeaderMap::new();
    headers.append("ORIGIN", HeaderValue::from_str("example.com").unwrap());
    let responder = Responder::new(config, headers).unwrap();
    let builder = responder.response_builder();
    let built_headers = builder.headers_ref().unwrap();
    assert_eq!(built_headers.get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "*");
    assert_eq!(
        built_headers.get(ACCESS_CONTROL_ALLOW_METHODS).unwrap(),
        "GET"
    );
}

#[test]
fn responder_config_reads_from_file() {
    let config = ResponderConfig::from_path("Config.toml");
    assert!(config.allowed_methods.is_some());
    assert!(config.allowed_origins.is_some());
}

#[tokio::test]
async fn main_starts_server() {
    let _handle = std::thread::spawn(move || {
        main();
    });
    sleep(Duration::new(1, 0));
    let client = Client::new();
    let request_builder = Request::builder()
        .uri("http://localhost:8477")
        .method(Method::OPTIONS)
        .body(Body::empty())
        .unwrap();
    let response = client.request(request_builder).await;
    assert_eq!(response.unwrap().status(), 200);
}
