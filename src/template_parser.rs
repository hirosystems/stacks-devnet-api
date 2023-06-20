use crate::resources::{
    configmap::StacksDevnetConfigmap, pod::StacksDevnetPod, pvc::StacksDevnetPvc,
    service::StacksDevnetService, StacksDevnetResource,
};

pub fn get_yaml_from_resource(resource: StacksDevnetResource) -> &'static str {
    match resource {
        StacksDevnetResource::Pod(StacksDevnetPod::BitcoindNode) => {
            include_str!("../templates/bitcoind-chain-coordinator-pod.template.yaml")
        }
        StacksDevnetResource::Service(StacksDevnetService::BitcoindNode) => {
            include_str!("../templates/bitcoind-chain-coordinator-service.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::BitcoindNode) => {
            include_str!("../templates/bitcoind-configmap.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::DeploymentPlan) => {
            include_str!("../templates/chain-coord-deployment-plan-configmap.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::Devnet) => {
            include_str!("../templates/chain-coord-devnet-configmap.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::Namespace) => {
            include_str!("../templates/chain-coord-namespace-configmap.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::ProjectDir) => {
            include_str!("../templates/chain-coord-project-dir-configmap.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::ProjectManifest) => {
            include_str!("../templates/chain-coord-project-manifest-configmap.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::StacksApi) => {
            include_str!("../templates/stacks-api-configmap.template.yaml")
        }
        StacksDevnetResource::Pod(StacksDevnetPod::StacksApi) => {
            include_str!("../templates/stacks-api-pod.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::StacksApiPostgres) => {
            include_str!("../templates/stacks-api-postgres-configmap.template.yaml")
        }
        StacksDevnetResource::Pvc(StacksDevnetPvc::StacksApi) => {
            include_str!("../templates/stacks-api-pvc.template.yaml")
        }
        StacksDevnetResource::Service(StacksDevnetService::StacksApi) => {
            include_str!("../templates/stacks-api-service.template.yaml")
        }
        StacksDevnetResource::Configmap(StacksDevnetConfigmap::StacksNode) => {
            include_str!("../templates/stacks-node-configmap.template.yaml")
        }
        StacksDevnetResource::Pod(StacksDevnetPod::StacksNode) => {
            include_str!("../templates/stacks-node-pod.template.yaml")
        }
        StacksDevnetResource::Service(StacksDevnetService::StacksNode) => {
            include_str!("../templates/stacks-node-service.template.yaml")
        }
        StacksDevnetResource::Namespace => include_str!("../templates/namespace.template.yaml"),
    }
}
