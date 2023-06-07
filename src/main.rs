use std::str::FromStr;

use serde::Deserialize;
use stacks_devnet_api::{delete_devnet, deploy_devnet, StacksDevnetConfig};
use tiny_http::{Method, Response, Server, StatusCode};
use url::Url;

#[derive(Deserialize, Debug)]
struct DeleteRequest {
    network: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    const HOST: &str = "127.0.0.1";
    const PORT: &str = "8477";
    let endpoint: String = HOST.to_owned() + ":" + PORT;

    let server = Server::http(&endpoint).unwrap();
    loop {
        // blocks until the next request is received
        let mut request = match server.recv() {
            Ok(rq) => rq,
            Err(e) => {
                println!("error: {}", e);
                break;
            }
        };

        let url = request.url();
        let full_url = format!("http://{}{}", &endpoint, url);
        let url = Url::from_str(&full_url)?;
        match request.method() {
            Method::Post => match url.path() {
                "/api/v1/networks" => {
                    let mut content = String::new();
                    request.as_reader().read_to_string(&mut content).unwrap();
                    let config: StacksDevnetConfig = serde_json::from_str(&content)?;
                    match deploy_devnet(config).await {
                        Ok(_) => request.respond(Response::empty(200))?,
                        Err(e) => {
                            let status_code = match e.code {
                                _ => StatusCode::from(e.code),
                            };
                            // todo: there's got to be a better way to make a response
                            let response = Response::from_string(e.message);
                            request.respond(Response::with_status_code(response, status_code))?
                        }
                    }
                }
                _ => request.respond(Response::empty(404))?,
            },
            Method::Delete => match url.path() {
                "/api/v1/network" => {
                    if let Some(query) = url.query() {
                        let delete_request: DeleteRequest = serde_qs::from_str(query)?;
                        delete_devnet(&delete_request.network).await?;
                        request.respond(Response::empty(200))?
                    } else {
                        request.respond(Response::empty(400))?;
                    }
                }
                _ => request.respond(Response::empty(404))?,
            },
            // TODO: respond with unimplemented
            _ => request.respond(Response::empty(501))?,
        }
    }

    Ok(())
}
