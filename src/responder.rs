use std::convert::Infallible;

use hyper::{
    header::ACCESS_CONTROL_ALLOW_ORIGIN,
    http::{response::Builder, HeaderValue},
    Body, HeaderMap, Response, StatusCode,
};

#[derive(Default)]
pub struct Responder {
    pub allowed_hosts: Vec<String>,
    pub headers: HeaderMap<HeaderValue>,
}

impl Responder {
    pub fn response_builder(&self) -> Builder {
        let mut builder = Response::builder();
        match self.headers.get(ACCESS_CONTROL_ALLOW_ORIGIN) {
            Some(header_value) => {
                for allowed_host in &self.allowed_hosts {
                    if header_value == allowed_host {
                        builder = builder.header(ACCESS_CONTROL_ALLOW_ORIGIN, allowed_host);
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
