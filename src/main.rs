use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Method, Request, Response, Server, StatusCode, Uri};
use serde::Deserialize;
use stacks_devnet_api::{delete_devnet, deploy_devnet, StacksDevnetConfig};
use std::net::IpAddr;
use std::str::FromStr;
use std::{convert::Infallible, net::SocketAddr};

#[derive(Deserialize, Debug)]
struct DevnetRequestQueryPararms {
    network: String,
}

#[tokio::main]
async fn main() {
    const HOST: &str = "127.0.0.1";
    const PORT: &str = "8477";
    let endpoint: String = HOST.to_owned() + ":" + PORT;
    let addr: SocketAddr = endpoint.parse().expect("Could not parse ip:port.");

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let remote_addr = conn.remote_addr().ip();
        async move { Ok::<_, Infallible>(service_fn(move |req| handle(remote_addr, req))) }
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Running server on {:?}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

async fn proxy(
    mut request: Request<Body>,
    proxy_data: ProxyData,
) -> Result<Response<Body>, Infallible> {
    let uri = request.uri();

    if let Some(query) = uri.query() {
        let params: DevnetRequestQueryPararms = serde_qs::from_str(query).unwrap();
        let _network = &params.network;
        let forward_url = format!("http://127.0.0.1:{}", proxy_data.destination_service); // format!("http://{}.{}.svc.cluster.local", proxy_data.destination_service, network);

        *request.uri_mut() = {
            let forward_uri = format!("{}{}?{}", forward_url, proxy_data.path_to_forward, query);
            Uri::from_str(forward_uri.as_str())
        }
        .unwrap();
        let client = Client::new();

        match client.request(request).await {
            Ok(response) => Ok(response),
            Err(_error) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()),
        }
    } else {
        Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap())
    }
}

struct ProxyData {
    path_to_forward: String,
    destination_service: String,
}
fn get_proxy_data(path: &str) -> Option<ProxyData> {
    const BITCOIN_NODE_PATH: &str = "/api/v1/network/bitcoin-node";
    const BITCOIN_NODE_SERVICE: &str = "18443";
    const STACKS_NODE_PATH: &str = "/api/v1/network/stacks-node";
    const STACKS_NODE_SERVICE: &str = "20443";
    const STACKS_API_PATH: &str = "/api/v1/network/stacks-api";
    const STACKS_API_SERVICE: &str = "3999";

    if path.starts_with(BITCOIN_NODE_PATH) {
        return Some(ProxyData {
            path_to_forward: path.replace(BITCOIN_NODE_PATH, ""),
            destination_service: BITCOIN_NODE_SERVICE.into(),
        });
    } else if path.starts_with(STACKS_NODE_PATH) {
        return Some(ProxyData {
            path_to_forward: path.replace(STACKS_NODE_PATH, ""),
            destination_service: STACKS_NODE_SERVICE.into(),
        });
    } else if path.starts_with(STACKS_API_PATH) {
        return Some(ProxyData {
            path_to_forward: path.replace(STACKS_API_PATH, ""),
            destination_service: STACKS_API_SERVICE.into(),
        });
    }
    None
}

async fn handle(_client_ip: IpAddr, request: Request<Body>) -> Result<Response<Body>, Infallible> {
    let uri = request.uri();
    let path = uri.path();

    match request.method() {
        &Method::POST => {
            if path == "/api/v1/networks" {
                let body = hyper::body::to_bytes(request.into_body()).await;
                if body.is_err() {
                    return Ok(Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                        .unwrap());
                }
                let body = body.unwrap();
                let config: StacksDevnetConfig = serde_json::from_slice(&body).unwrap();
                match deploy_devnet(config).await {
                    Ok(_) => Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::empty())
                        .unwrap()),
                    Err(e) => Ok(Response::builder()
                        .status(StatusCode::from_u16(e.code).unwrap())
                        .body(Body::try_from(e.message).unwrap())
                        .unwrap()),
                }
            } else {
                let proxy_data = get_proxy_data(path);
                match proxy_data {
                    Some(proxy_data) => proxy(request, proxy_data).await,
                    None => Ok(Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Body::empty())
                        .unwrap()),
                }
            }
        }
        &Method::GET => {
            let proxy_data = get_proxy_data(path);
            match proxy_data {
                Some(proxy_data) => proxy(request, proxy_data).await,
                None => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap()),
            }
        }
        &Method::DELETE => match uri.path() {
            "/api/v1/network" => {
                if let Some(query) = uri.query() {
                    let delete_request: DevnetRequestQueryPararms =
                        serde_qs::from_str(query).unwrap();
                    match delete_devnet(&delete_request.network).await {
                        Ok(_) => Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(Body::empty())
                            .unwrap()),
                        Err(_e) => Ok(Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::empty())
                            .unwrap()),
                    }
                } else {
                    Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::empty())
                        .unwrap())
                }
            }
            _ => Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap()),
        },
        _ => Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .unwrap()),
    }
}
