use super::{
    pvc::StacksDevnetPvc,
    service::{get_service_from_path_part, get_service_port, get_user_facing_port, ServicePort},
    StacksDevnetConfigmap, StacksDevnetPod, StacksDevnetService,
};
use test_case::test_case;

#[test_case(StacksDevnetConfigmap::BitcoindNode => is equal_to  "bitcoind-conf".to_string(); "for BitcoinNode")]
#[test_case(StacksDevnetConfigmap::StacksNode => is equal_to  "stacks-node-conf".to_string(); "for StacksNode")]
#[test_case(StacksDevnetConfigmap::StacksApi => is equal_to  "stacks-api-conf".to_string(); "for StacksApi")]
#[test_case(StacksDevnetConfigmap::StacksApiPostgres => is equal_to  "stacks-api-postgres-conf".to_string(); "for StacksApiPostgres")]
#[test_case(StacksDevnetConfigmap::DeploymentPlan => is equal_to  "deployment-plan-conf".to_string(); "for DeploymentPlan")]
#[test_case(StacksDevnetConfigmap::Devnet => is equal_to  "devnet-conf".to_string(); "for Devnet")]
#[test_case(StacksDevnetConfigmap::ProjectDir => is equal_to  "project-dir-conf".to_string(); "for ProjectDir")]
#[test_case(StacksDevnetConfigmap::Namespace => is equal_to  "namespace-conf".to_string(); "for Namespace")]
#[test_case(StacksDevnetConfigmap::ProjectManifest => is equal_to  "project-manifest-conf".to_string(); "for ProjectManifest")]
fn it_prints_correct_name_for_configmap(configmap: StacksDevnetConfigmap) -> String {
    configmap.to_string()
}

#[test_case(StacksDevnetPod::BitcoindNode => is equal_to  "bitcoind-chain-coordinator".to_string(); "for BitcoindNode")]
#[test_case(StacksDevnetPod::StacksNode => is equal_to  "stacks-node".to_string(); "for StacksNode")]
#[test_case(StacksDevnetPod::StacksApi => is equal_to  "stacks-api".to_string(); "for StacksApi")]
fn it_prints_correct_name_for_pod(pod: StacksDevnetPod) -> String {
    pod.to_string()
}

#[test_case(StacksDevnetPvc::StacksApi => is equal_to  "stacks-api-pvc".to_string(); "for StacksApi")]
fn it_prints_correct_name_for_pvc(pvc: StacksDevnetPvc) -> String {
    pvc.to_string()
}

#[test_case(StacksDevnetService::BitcoindNode => is equal_to  "bitcoind-chain-coordinator-service".to_string(); "for BitcoindNode")]
#[test_case(StacksDevnetService::StacksNode => is equal_to  "stacks-node-service".to_string(); "for StacksNode")]
#[test_case(StacksDevnetService::StacksApi => is equal_to  "stacks-api-service".to_string(); "for StacksApi")]
fn it_prints_correct_name_for_service(service: StacksDevnetService) -> String {
    service.to_string()
}

#[test_case(StacksDevnetService::BitcoindNode, ServicePort::RPC => is equal_to  Some("18443".to_string()); "for BitcoindNode RPC port")]
#[test_case(StacksDevnetService::BitcoindNode, ServicePort::P2P => is equal_to  Some("18444".to_string()); "for BitcoindNode P2P port")]
#[test_case(StacksDevnetService::BitcoindNode, ServicePort::Ingestion => is equal_to  Some("20445".to_string()); "for BitcoindNode Ingestion port")]
#[test_case(StacksDevnetService::BitcoindNode, ServicePort::Control => is equal_to  Some("20446".to_string()); "for BitcoindNode Control port")]
#[test_case(StacksDevnetService::StacksNode, ServicePort::RPC => is equal_to  Some("20443".to_string()); "for StacksNode RPC port")]
#[test_case(StacksDevnetService::StacksNode, ServicePort::P2P => is equal_to  Some("20444".to_string()); "for StacksNode P2P port")]
#[test_case(StacksDevnetService::StacksApi, ServicePort::API => is equal_to  Some("3999".to_string()); "for StacksApi API port")]
#[test_case(StacksDevnetService::StacksApi, ServicePort::Event => is equal_to  Some("3700".to_string()); "for StacksApi Event port")]
#[test_case(StacksDevnetService::StacksApi, ServicePort::DB => is equal_to  Some("5432".to_string()); "for StacksApi DB port")]
#[test_case(StacksDevnetService::StacksApi, ServicePort::RPC => is equal_to  None; "invalid service port combination")]
fn it_gets_correct_port_for_service(
    service: StacksDevnetService,
    port_type: ServicePort,
) -> Option<String> {
    get_service_port(service, port_type)
}

#[test_case("bitcoin-node" => is equal_to Some(StacksDevnetService::BitcoindNode); "for bitcoin-node")]
#[test_case("stacks-node" => is equal_to Some(StacksDevnetService::StacksNode); "for stacks-node")]
#[test_case("stacks-api" => is equal_to Some(StacksDevnetService::StacksApi); "for stacks-api")]
#[test_case("invalid" => is equal_to None; "returning None for invalid paths")]
fn it_prints_service_from_path_part(path_part: &str) -> Option<StacksDevnetService> {
    get_service_from_path_part(path_part)
}

#[test_case(StacksDevnetService::BitcoindNode => is equal_to  Some("18443".to_string()); "for BitcoindNode")]
#[test_case(StacksDevnetService::StacksNode => is equal_to  Some("20443".to_string()); "for StacksNode")]
#[test_case(StacksDevnetService::StacksApi => is equal_to  Some("3999".to_string()); "for StacksApi")]
fn it_gets_user_facing_port_for_service(service: StacksDevnetService) -> Option<String> {
    get_user_facing_port(service)
}
