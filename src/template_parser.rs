use crate::resources::{
    configmap::StacksDevnetConfigmap, pod::StacksDevnetPod, pvc::StacksDevnetPvc,
    service::StacksDevnetService, StacksDevnetResource,
};

pub fn get_yaml_from_resource(resource: StacksDevnetResource) -> &'static str {
    match resource {
        StacksDevnetResource::Pod(StacksDevnetPod::BitcoindNode) => {
            include_str!("../templates/pods/bitcoind-chain-coordinator.template.yaml")
        }
        StacksDevnetResource::Service(StacksDevnetService::BitcoindNode) => {
            include_str!("../templates/services/bitcoind-chain-coordinator.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::BitcoindNode) => {
            include_str!("../templates/configmaps/bitcoind.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::DeploymentPlan) => {
            include_str!("../templates/configmaps/chain-coord-deployment-plan.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::Devnet) => {
            include_str!("../templates/configmaps/chain-coord-devnet.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::ProjectDir) => {
            include_str!("../templates/configmaps/chain-coord-project-dir.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::ProjectManifest) => {
            include_str!("../templates/configmaps/chain-coord-project-manifest.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::StacksBlockchainApi) => {
            include_str!("../templates/configmaps/stacks-blockchain-api.template.yaml")
        }
        StacksDevnetResource::Pod(StacksDevnetPod::StacksBlockchainApi) => {
            include_str!("../templates/pods/stacks-blockchain-api.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::StacksBlockchainApiPg) => {
            include_str!("../templates/configmaps/stacks-blockchain-api-pg.template.yaml")
        }
        StacksDevnetResource::Pvc(StacksDevnetPvc::StacksBlockchainApiPg) => {
            include_str!("../templates/pvcs/stacks-blockchain-api-pg.template.yaml")
        }
        StacksDevnetResource::Service(StacksDevnetService::StacksBlockchainApi) => {
            include_str!("../templates/services/stacks-blockchain-api.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::StacksBlockchain) => {
            include_str!("../templates/configmaps/stacks-blockchain.template.yaml")
        }
        StacksDevnetResource::Pod(StacksDevnetPod::StacksBlockchain) => {
            include_str!("../templates/pods/stacks-blockchain.template.yaml")
        }
        StacksDevnetResource::Service(StacksDevnetService::StacksBlockchain) => {
            include_str!("../templates/services/stacks-blockchain.template.yaml")
        }
        StacksDevnetResource::Namespace => include_str!("../templates/namespace.template.yaml"),
    }
}
