use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug, Clone)]
pub enum StacksDevnetService {
    BitcoindNode,
    StacksNode,
    StacksApi,
}

pub enum ServicePort {
    RPC,
    P2P,
    Ingestion,
    Control,
    Event,
    API,
    DB,
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

pub fn get_service_port(service: StacksDevnetService, port_type: ServicePort) -> Option<String> {
    match (service, port_type) {
        (StacksDevnetService::BitcoindNode, ServicePort::RPC) => Some("18443".into()),
        (StacksDevnetService::BitcoindNode, ServicePort::P2P) => Some("18444".into()),
        (StacksDevnetService::BitcoindNode, ServicePort::Ingestion) => Some("20445".into()),
        (StacksDevnetService::BitcoindNode, ServicePort::Control) => Some("20446".into()),
        (StacksDevnetService::StacksNode, ServicePort::RPC) => Some("20443".into()),
        (StacksDevnetService::StacksNode, ServicePort::P2P) => Some("20444".into()),
        (StacksDevnetService::StacksApi, ServicePort::API) => Some("3999".into()),
        (StacksDevnetService::StacksApi, ServicePort::Event) => Some("3700".into()),
        (StacksDevnetService::StacksApi, ServicePort::DB) => Some("5432".into()),
        (_, _) => None,
    }
}

pub fn get_user_facing_port(service: StacksDevnetService) -> Option<String> {
    match service {
        StacksDevnetService::BitcoindNode | StacksDevnetService::StacksNode => {
            get_service_port(service, ServicePort::RPC)
        }
        StacksDevnetService::StacksApi => get_service_port(service, ServicePort::API),
    }
}

pub fn get_service_url(namespace: &str, service: StacksDevnetService) -> String {
    format!("{}.{}.svc.cluster.local", service.to_string(), namespace)
}

pub fn get_service_from_path_part(path_part: &str) -> Option<StacksDevnetService> {
    match path_part {
        "bitcoin-node" => Some(StacksDevnetService::BitcoindNode),
        "stacks-node" => Some(StacksDevnetService::StacksNode),
        "stacks-api" => Some(StacksDevnetService::StacksApi),
        _ => None,
    }
}
