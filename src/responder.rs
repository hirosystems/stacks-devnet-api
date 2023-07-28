use std::{
    convert::Infallible,
    fs::File,
    io::{BufReader, Read},
};

use hyper::{
    header::{ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN},
    http::{response::Builder, HeaderValue},
    Body, HeaderMap, Response, StatusCode,
};
use serde::{Deserialize, Serialize};

#[derive(Default)]
pub struct Responder {
    pub allowed_origins: Vec<String>,
    allowed_methods: Vec<String>,
    pub headers: HeaderMap<HeaderValue>,
}

#[derive(Serialize, Deserialize)]
pub struct StacksDevnetStartupConfig {
    allowed_origins: Option<Vec<String>>,
    allowed_methods: Option<Vec<String>>,
}

impl Responder {
    pub fn new(
        config_location: &str,
        headers: HeaderMap<HeaderValue>,
    ) -> Result<Responder, String> {
        let file = File::open(config_location)
            .map_err(|e| format!("unable to read file {}\n{:?}", config_location, e))?;
        let mut file_reader = BufReader::new(file);
        let mut file_buffer = vec![];
        file_reader
            .read_to_end(&mut file_buffer)
            .map_err(|e| format!("unable to read file {}\n{:?}", config_location, e))?;

        let config_file: StacksDevnetStartupConfig = match toml::from_slice(&file_buffer) {
            Ok(s) => s,
            Err(e) => {
                return Err(format!("Config file malformatted {}", e.to_string()));
            }
        };
        Ok(Responder {
            allowed_origins: config_file.allowed_origins.unwrap_or_default(),
            allowed_methods: config_file.allowed_methods.unwrap_or_default(),
            headers,
        })
    }

    pub fn response_builder(&self) -> Builder {
        let mut builder = Response::builder();

        for method in &self.allowed_methods {
            builder = builder.header(ACCESS_CONTROL_ALLOW_METHODS, method);
        }

        match self.headers.get(ORIGIN) {
            Some(header_value) => {
                if self.allowed_origins.clone().into_iter().any(|h| h == "*") {
                    builder = builder.header(ACCESS_CONTROL_ALLOW_ORIGIN, "*");
                    return builder;
                }
                for allowed_host in &self.allowed_origins {
                    if header_value == allowed_host {
                        builder = builder.header(ACCESS_CONTROL_ALLOW_ORIGIN, allowed_host);
                        break;
                    }
                }
                return builder;
            }
            None => builder,
        }
    }

    fn _respond(&self, code: StatusCode, body: String) -> Result<Response<Body>, Infallible> {
        let builder = self.response_builder();
        match builder.status(code).body(Body::try_from(body).unwrap()) {
            Ok(r) => Ok(r),
            Err(_) => unreachable!(),
        }
    }

    pub fn respond(&self, code: u16, body: String) -> Result<Response<Body>, Infallible> {
        self._respond(StatusCode::from_u16(code).unwrap(), body)
    }

    pub fn ok(&self) -> Result<Response<Body>, Infallible> {
        self._respond(StatusCode::OK, "Ok".into())
    }

    pub fn err_method_not_allowed(&self, body: String) -> Result<Response<Body>, Infallible> {
        self._respond(StatusCode::METHOD_NOT_ALLOWED, body)
    }

    pub fn err_bad_request(&self, body: String) -> Result<Response<Body>, Infallible> {
        self._respond(StatusCode::BAD_REQUEST, body)
    }

    pub fn err_not_found(&self, body: String) -> Result<Response<Body>, Infallible> {
        self._respond(StatusCode::NOT_FOUND, body)
    }

    pub fn err_not_implemented(&self, body: String) -> Result<Response<Body>, Infallible> {
        self._respond(StatusCode::NOT_FOUND, body)
    }

    pub fn err_internal(&self, body: String) -> Result<Response<Body>, Infallible> {
        self._respond(StatusCode::INTERNAL_SERVER_ERROR, body)
    }
}
