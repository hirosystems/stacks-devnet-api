use std::{
    fs::File,
    io::{BufReader, Read},
};

use serde::{Deserialize, Serialize};

#[derive(Serialize, serde::Deserialize, Clone, Default)]
pub struct ApiConfig {
    #[serde(rename = "http_response")]
    pub http_response_config: ResponderConfig,
    #[serde(rename = "auth")]
    pub auth_config: AuthConfig,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ResponderConfig {
    pub allowed_origins: Option<Vec<String>>,
    pub allowed_methods: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AuthConfig {
    pub auth_header: Option<String>,
}

impl ApiConfig {
    pub fn from_path(config_path: &str) -> ApiConfig {
        let file = File::open(config_path)
            .unwrap_or_else(|e| panic!("unable to read file {}\n{:?}", config_path, e));
        let mut file_reader = BufReader::new(file);
        let mut file_buffer = vec![];
        file_reader
            .read_to_end(&mut file_buffer)
            .unwrap_or_else(|e| panic!("unable to read file {}\n{:?}", config_path, e));

        let config_file: ApiConfig = match toml::from_slice(&file_buffer) {
            Ok(s) => s,
            Err(e) => {
                panic!("Config file malformatted {}", e.to_string());
            }
        };
        config_file
    }
}
