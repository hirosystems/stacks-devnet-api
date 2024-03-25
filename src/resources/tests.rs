use super::{
    deployment::StacksDevnetDeployment,
    service::{get_service_from_path_part, get_service_port, get_user_facing_port, ServicePort},
    stateful_set::StacksDevnetStatefulSet,
    StacksDevnetConfigmap, StacksDevnetPod, StacksDevnetService,
};
use test_case::test_case;

#[test_case(StacksDevnetConfigmap::BitcoindNode => is equal_to  "bitcoind".to_string(); "for BitcoinNode")]
#[test_case(StacksDevnetConfigmap::StacksBlockchain => is equal_to  "stacks-blockchain".to_string(); "for StacksBlockchain")]
#[test_case(StacksDevnetConfigmap::StacksBlockchainApi => is equal_to  "stacks-blockchain-api".to_string(); "for StacksBlockchainApi")]
#[test_case(StacksDevnetConfigmap::StacksBlockchainApiPg => is equal_to  "stacks-blockchain-api-pg".to_string(); "for StacksBlockchainApiPg")]
#[test_case(StacksDevnetConfigmap::StacksSigner0 => is equal_to  "stacks-signer-0".to_string(); "for StacksSigner0")]
#[test_case(StacksDevnetConfigmap::StacksSigner1 => is equal_to  "stacks-signer-1".to_string(); "for StacksSigner1")]
#[test_case(StacksDevnetConfigmap::DeploymentPlan => is equal_to  "deployment-plan".to_string(); "for DeploymentPlan")]
#[test_case(StacksDevnetConfigmap::Devnet => is equal_to  "devnet".to_string(); "for Devnet")]
#[test_case(StacksDevnetConfigmap::ProjectDir => is equal_to  "project-dir".to_string(); "for ProjectDir")]
#[test_case(StacksDevnetConfigmap::ProjectManifest => is equal_to  "project-manifest".to_string(); "for ProjectManifest")]
fn it_prints_correct_name_for_configmap(configmap: StacksDevnetConfigmap) -> String {
    configmap.to_string()
}

#[test_case(StacksDevnetPod::BitcoindNode => is equal_to  "bitcoind-chain-coordinator".to_string(); "for BitcoindNode")]
#[test_case(StacksDevnetPod::StacksBlockchain => is equal_to  "stacks-blockchain".to_string(); "for StacksBlockchain")]
#[test_case(StacksDevnetPod::StacksBlockchainApi => is equal_to  "stacks-blockchain-api".to_string(); "for StacksBlockchainApi")]
fn it_prints_correct_name_for_pod(pod: StacksDevnetPod) -> String {
    pod.to_string()
}

#[test_case(StacksDevnetDeployment::BitcoindNode => is equal_to  "bitcoind-chain-coordinator".to_string(); "for BitcoindNode")]
#[test_case(StacksDevnetDeployment::StacksBlockchain => is equal_to  "stacks-blockchain".to_string(); "for StacksBlockchain")]
fn it_prints_correct_name_for_deployment(deployment: StacksDevnetDeployment) -> String {
    deployment.to_string()
}

#[test_case(StacksDevnetStatefulSet::StacksBlockchainApi => is equal_to  "stacks-blockchain-api".to_string(); "for StacksBlockchainApi")]
#[test_case(StacksDevnetStatefulSet::StacksSigner0 => is equal_to  "stacks-signer-0".to_string(); "for StacksSigner0")]
#[test_case(StacksDevnetStatefulSet::StacksSigner1 => is equal_to  "stacks-signer-1".to_string(); "for StacksSigner1")]
fn it_prints_correct_name_for_stateful_set(pod: StacksDevnetStatefulSet) -> String {
    pod.to_string()
}

#[test_case(StacksDevnetService::BitcoindNode => is equal_to  "bitcoind-chain-coordinator".to_string(); "for BitcoindNode")]
#[test_case(StacksDevnetService::StacksBlockchain => is equal_to  "stacks-blockchain".to_string(); "for StacksBlockchain")]
#[test_case(StacksDevnetService::StacksBlockchainApi => is equal_to  "stacks-blockchain-api".to_string(); "for StacksBlockchainApi")]
#[test_case(StacksDevnetService::StacksSigner0 => is equal_to  "stacks-signer-0".to_string(); "for StacksSigner0")]
#[test_case(StacksDevnetService::StacksSigner1 => is equal_to  "stacks-signer-1".to_string(); "for StacksSigner1")]
fn it_prints_correct_name_for_service(service: StacksDevnetService) -> String {
    service.to_string()
}

#[test_case(StacksDevnetService::BitcoindNode, ServicePort::RPC => is equal_to  Some("18443".to_string()); "for BitcoindNode RPC port")]
#[test_case(StacksDevnetService::BitcoindNode, ServicePort::P2P => is equal_to  Some("18444".to_string()); "for BitcoindNode P2P port")]
#[test_case(StacksDevnetService::BitcoindNode, ServicePort::Ingestion => is equal_to  Some("20445".to_string()); "for BitcoindNode Ingestion port")]
#[test_case(StacksDevnetService::BitcoindNode, ServicePort::Control => is equal_to  Some("20446".to_string()); "for BitcoindNode Control port")]
#[test_case(StacksDevnetService::StacksBlockchain, ServicePort::RPC => is equal_to  Some("20443".to_string()); "for StacksBlockchain RPC port")]
#[test_case(StacksDevnetService::StacksBlockchain, ServicePort::P2P => is equal_to  Some("20444".to_string()); "for StacksBlockchain P2P port")]
#[test_case(StacksDevnetService::StacksBlockchainApi, ServicePort::API => is equal_to  Some("3999".to_string()); "for StacksBlockchainApi API port")]
#[test_case(StacksDevnetService::StacksBlockchainApi, ServicePort::Event => is equal_to  Some("3700".to_string()); "for StacksBlockchainApi Event port")]
#[test_case(StacksDevnetService::StacksBlockchainApi, ServicePort::DB => is equal_to  Some("5432".to_string()); "for StacksBlockchainApi DB port")]
#[test_case(StacksDevnetService::StacksSigner0, ServicePort::Event => is equal_to  Some("30001".to_string()); "for StacksSigner0 Event port")]
#[test_case(StacksDevnetService::StacksSigner1, ServicePort::Event => is equal_to  Some("30001".to_string()); "for StacksSigner1 Event port")]
#[test_case(StacksDevnetService::StacksBlockchainApi, ServicePort::RPC => is equal_to  None; "invalid service port combination")]
fn it_gets_correct_port_for_service(
    service: StacksDevnetService,
    port_type: ServicePort,
) -> Option<String> {
    get_service_port(service, port_type)
}

#[test_case("bitcoin-node" => is equal_to Some(StacksDevnetService::BitcoindNode); "for bitcoin-node")]
#[test_case("stacks-blockchain" => is equal_to Some(StacksDevnetService::StacksBlockchain); "for stacks-blockchain")]
#[test_case("stacks-blockchain-api" => is equal_to Some(StacksDevnetService::StacksBlockchainApi); "for stacks-blockchain-api")]
#[test_case("stacks-signer-0" => is equal_to Some(StacksDevnetService::StacksSigner0); "for stacks-signer-0")]
#[test_case("stacks-signer-1" => is equal_to Some(StacksDevnetService::StacksSigner1); "for stacks-signer-1")]
#[test_case("invalid" => is equal_to None; "returning None for invalid paths")]
fn it_prints_service_from_path_part(path_part: &str) -> Option<StacksDevnetService> {
    get_service_from_path_part(path_part)
}

#[test_case(StacksDevnetService::BitcoindNode => is equal_to Some("18443".to_string()); "for BitcoindNode")]
#[test_case(StacksDevnetService::StacksBlockchain => is equal_to Some("20443".to_string()); "for StacksBlockchain")]
#[test_case(StacksDevnetService::StacksBlockchainApi => is equal_to Some("3999".to_string()); "for StacksBlockchainApi")]
#[test_case(StacksDevnetService::StacksSigner0 => is equal_to None; "for StacksSigner0")]
#[test_case(StacksDevnetService::StacksSigner1 => is equal_to None; "for StacksSigner1")]
fn it_gets_user_facing_port_for_service(service: StacksDevnetService) -> Option<String> {
    get_user_facing_port(service)
}
