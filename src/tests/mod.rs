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
use test_case::test_case;
use tower_test::mock::{self, Handle};

async fn mock_k8s_handler(handle: &mut Handle<Request<Body>, Response<Body>>) {
    let (request, send) = handle.next_request().await.expect("Service not called");
    println!("method {}", request.method().as_str());
    println!("path {}", request.uri().to_string().as_str());
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

async fn make_k8s_manager() -> (StacksDevnetApiK8sManager, Context) {
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
    (k8s_manager, ctx)
}

#[test_case("/path" ; "/path")]
#[test_case("/api" ; "/api")]
#[test_case("/api/v1" ; "/api/v1")]
#[test_case("/api/v1/network2" ; "/api/v1/network2")]
#[tokio::test]
async fn it_responds_400_for_invalid_paths(invalid_path: &str) {
    let (k8s_manager, ctx) = make_k8s_manager().await;

    let request_builder = Request::builder().uri(invalid_path).method("GET");
    let request: Request<Body> = request_builder.body(Body::empty()).unwrap();
    let mut response = handle_request(
        request,
        k8s_manager.clone(),
        ResponderConfig::default(),
        ctx.clone(),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.body_mut();
    let bytes = body::to_bytes(body).await.unwrap().to_vec();
    let body_str = String::from_utf8(bytes).unwrap();
    assert_eq!(body_str, "invalid request path");
}

#[test_case("any", Method::OPTIONS, None => 
    is equal_to (StatusCode::OK, "Ok".to_string()); "200 for any OPTIONS request")]
#[test_case("/api/v1/network/undeployed", Method::GET, None => 
        is equal_to (StatusCode::NOT_FOUND, "network undeployed does not exist".to_string()); "404 for undeployed namespace")]
#[test_case("/api/v1/network/test", Method::GET, None => 
is equal_to (StatusCode::NOT_FOUND, "network undeployed does not exist".to_string()); "what")]
#[test_case("/api/v1/network/500_err", Method::GET, None => 
    is equal_to (StatusCode::INTERNAL_SERVER_ERROR, "error getting namespace 500_err: \"\"".to_string()); "forwarded error if fetching namespace returns error")]
#[test_case("/api/v1/network/test", Method::POST, None => 
    is equal_to (StatusCode::METHOD_NOT_ALLOWED, "can only GET/DELETE/HEAD at provided route".to_string()); "405 for network route with POST request")]
#[test_case("/api/v1/network/test/commands", Method::GET, None => 
is equal_to (StatusCode::NOT_FOUND, "commands route in progress".to_string()); "404 for network commands route")]
#[test_case("/api/v1/network/", Method::GET, None => 
        is equal_to (StatusCode::BAD_REQUEST, "no network id provided".to_string()); "400 for missing namespace")]
#[test_case("/api/v1/networks", Method::GET, None => 
        is equal_to (StatusCode::METHOD_NOT_ALLOWED, "network creation must be a POST request".to_string()); "405 for network creation request with GET method")]
#[test_case("/api/v1/networks", Method::DELETE, None => 
        is equal_to (StatusCode::METHOD_NOT_ALLOWED, "network creation must be a POST request".to_string()); "405 for network creation request with DELETE method")]
#[test_case("/api/v1/networks", Method::POST, None => 
        is equal_to (StatusCode::BAD_REQUEST, "invalid configuration to create network: EOF while parsing a value at line 1 column 0".to_string()); "400 for network creation request invalid config")]
#[tokio::test]
async fn it_responds_to_requests(
    request_path: &str,
    method: Method,
    body: Option<Body>,
) -> (StatusCode, String) {
    let (k8s_manager, ctx) = make_k8s_manager().await;

    let request_builder = Request::builder().uri(request_path).method(method);
    let body = match body {
        Some(b) => b,
        None => Body::empty(),
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
    let body_str = String::from_utf8(bytes).unwrap();
    println!("{}", body_str);
    (response.status(), body_str)
}

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
