use hiro_system_kit::slog;
use hyper::{
    header::{
        ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
        ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN,
    },
    http::{response::Builder, HeaderValue},
    Body, HeaderMap, Response, StatusCode,
};
use std::convert::Infallible;

use crate::{api_config::ResponderConfig, Context};

pub struct Responder {
    allowed_origins: Vec<String>,
    allowed_methods: Vec<String>,
    allowed_headers: String,
    headers: HeaderMap<HeaderValue>,
    ctx: Context,
}

impl Default for Responder {
    fn default() -> Self {
        Responder {
            allowed_origins: Vec::default(),
            allowed_methods: Vec::default(),
            allowed_headers: String::default(),
            headers: HeaderMap::default(),
            ctx: Context::empty(),
        }
    }
}
impl Responder {
    pub fn new(
        config: ResponderConfig,
        headers: HeaderMap<HeaderValue>,
        ctx: Context,
    ) -> Result<Responder, String> {
        Ok(Responder {
            allowed_origins: config.allowed_origins.unwrap_or_default(),
            allowed_methods: config.allowed_methods.unwrap_or_default(),
            allowed_headers: config.allowed_headers.unwrap_or("*".to_string()),
            headers,
            ctx,
        })
    }

    pub fn response_builder(&self) -> Builder {
        let mut builder = Response::builder()
            .header(ACCESS_CONTROL_ALLOW_CREDENTIALS, "true")
            .header(ACCESS_CONTROL_ALLOW_HEADERS, &self.allowed_headers);

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
        let body = match Body::try_from(body) {
            Ok(b) => b,
            Err(e) => {
                self.ctx.try_log(|logger| {
                    slog::error!(
                        logger,
                        "responder failed to create response body: {}",
                        e.to_string()
                    )
                });
                Body::empty()
            }
        };
        match builder.status(code).body(body) {
            Ok(r) => Ok(r),
            Err(e) => {
                self.ctx.try_log(|logger| {
                    slog::error!(
                        logger,
                        "responder failed to send response: {}",
                        e.to_string()
                    )
                });
                Ok(self
                    .response_builder()
                    .status(500)
                    .body(Body::empty())
                    .unwrap())
            }
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
