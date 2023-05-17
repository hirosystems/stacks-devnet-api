use k8s_experimentation::{deploy_devnet, StacksDevnetConfig};
use tiny_http::{Method, Response, Server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    const HOST: &str = "127.0.0.1";
    const PORT: &str = "8477";
    let endpoint: String = HOST.to_owned() + ":" + PORT;

    let server = Server::http(endpoint).unwrap();
    loop {
        // blocks until the next request is received
        let mut request = match server.recv() {
            Ok(rq) => rq,
            Err(e) => {
                println!("error: {}", e);
                break;
            }
        };

        match request.method() {
            Method::Post => {
                let url = request.url();
                println!("{}", url);
                match url {
                    "/api/v1/networks" => {
                        let mut content = String::new();
                        request.as_reader().read_to_string(&mut content).unwrap();
                        let config: StacksDevnetConfig = serde_json::from_str(&content)?;
                        deploy_devnet(config).await?;
                        request.respond(Response::empty(200))?
                    }
                    _ => request.respond(Response::empty(404))?,
                }
            }
            //let not_implemented = Response::new( StatusCode::from(501), request.headers(), "not implemented", None, None );
            _ => request.respond(Response::empty(501))?,
        }
    }

    Ok(())
}
