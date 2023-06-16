use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetService {
    BitcoindNode,
    StacksNode,
    StacksApi,
}

impl fmt::Display for StacksDevnetService {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetService::BitcoindNode => write!(f, "bitcoind-chain-coordinator-service"),
            StacksDevnetService::StacksNode => write!(f, "stacks-node-service"),
            StacksDevnetService::StacksApi => write!(f, "stacks-api-service"),
        }
    }
}

pub fn get_service_url(service: StacksDevnetService, namespace: &str) -> String {
    let port = match service {
        StacksDevnetService::BitcoindNode => "18443",
        StacksDevnetService::StacksNode => "20443",
        StacksDevnetService::StacksApi => "3999",
    };

    format!(
        "http://{}.{}/svc/cluster.local:{}",
        service.to_string(),
        namespace,
        port
    )
}

pub fn get_service_from_path_part(path_part: &str) -> Option<StacksDevnetService> {
    match path_part {
        "bitcoin-node" => Some(StacksDevnetService::BitcoindNode),
        "stacks-node" => Some(StacksDevnetService::StacksNode),
        "stacks-api" => Some(StacksDevnetService::StacksApi),
        _ => None,
    }
}
