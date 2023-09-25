use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug, Clone, PartialEq)]
pub enum StacksDevnetService {
    BitcoindNode,
    StacksBlockchain,
    StacksBlockchainApi,
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
            StacksDevnetService::BitcoindNode => write!(f, "bitcoind-chain-coordinator"),
            StacksDevnetService::StacksBlockchain => write!(f, "stacks-blockchain"),
            StacksDevnetService::StacksBlockchainApi => write!(f, "stacks-blockchain-api"),
        }
    }
}

pub fn get_service_port(service: StacksDevnetService, port_type: ServicePort) -> Option<String> {
    match (service, port_type) {
        (StacksDevnetService::BitcoindNode, ServicePort::RPC) => Some("18443".into()),
        (StacksDevnetService::BitcoindNode, ServicePort::P2P) => Some("18444".into()),
        (StacksDevnetService::BitcoindNode, ServicePort::Ingestion) => Some("20445".into()),
        (StacksDevnetService::BitcoindNode, ServicePort::Control) => Some("20446".into()),
        (StacksDevnetService::StacksBlockchain, ServicePort::RPC) => Some("20443".into()),
        (StacksDevnetService::StacksBlockchain, ServicePort::P2P) => Some("20444".into()),
        (StacksDevnetService::StacksBlockchainApi, ServicePort::API) => Some("3999".into()),
        (StacksDevnetService::StacksBlockchainApi, ServicePort::Event) => Some("3700".into()),
        (StacksDevnetService::StacksBlockchainApi, ServicePort::DB) => Some("5432".into()),
        (_, _) => None,
    }
}

pub fn get_user_facing_port(service: StacksDevnetService) -> Option<String> {
    match service {
        StacksDevnetService::BitcoindNode | StacksDevnetService::StacksBlockchain => {
            get_service_port(service, ServicePort::RPC)
        }
        StacksDevnetService::StacksBlockchainApi => get_service_port(service, ServicePort::API),
    }
}

pub fn get_service_url(namespace: &str, service: StacksDevnetService) -> String {
    format!("{}.{}.svc.cluster.local", service.to_string(), namespace)
}

pub fn get_service_from_path_part(path_part: &str) -> Option<StacksDevnetService> {
    match path_part {
        "bitcoin-node" => Some(StacksDevnetService::BitcoindNode),
        "stacks-blockchain" => Some(StacksDevnetService::StacksBlockchain),
        "stacks-blockchain-api" => Some(StacksDevnetService::StacksBlockchainApi),
        _ => None,
    }
}
