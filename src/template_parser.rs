pub enum Template {
    BitcoindChainCoordinatorPod,
    BitcoindChainCoordinatorService,
    BitcoindConfigmap,
    ChainCoordinatorDeploymentPlanConfigmap,
    ChainCoordinatorDevnetConfigmap,
    ChainCoordinatorNamespaceConfigmap,
    ChainCoordinatorProjectDirConfigmap,
    ChainCoordinatorProjectManifestConfigmap,
    Namespace,
    StacksApiConfigmap,
    StacksApiPod,
    StacksApiPostgresConfigmap,
    StacksApiPvc,
    StacksApiService,
    StacksNodeConfigmap,
    StacksNodePod,
    StacksNodeService,
}

pub fn get_yaml_from_filename(template_filename: Template) -> &'static str {
    match template_filename {
        Template::BitcoindChainCoordinatorPod => {
            include_str!("../templates/bitcoind-chain-coordinator-pod.template.yaml")
        }
        Template::BitcoindChainCoordinatorService => {
            include_str!("../templates/bitcoind-chain-coordinator-service.template.yaml")
        }
        Template::BitcoindConfigmap => {
            include_str!("../templates/bitcoind-configmap.template.yaml")
        }
        Template::ChainCoordinatorDeploymentPlanConfigmap => {
            include_str!("../templates/chain-coord-deployment-plan-configmap.template.yaml")
        }
        Template::ChainCoordinatorDevnetConfigmap => {
            include_str!("../templates/chain-coord-devnet-configmap.template.yaml")
        }
        Template::ChainCoordinatorNamespaceConfigmap => {
            include_str!("../templates/chain-coord-namespace-configmap.template.yaml")
        }
        Template::ChainCoordinatorProjectDirConfigmap => {
            include_str!("../templates/chain-coord-project-dir-configmap.template.yaml")
        }
        Template::ChainCoordinatorProjectManifestConfigmap => {
            include_str!("../templates/chain-coord-project-manifest-configmap.template.yaml")
        }
        Template::Namespace => include_str!("../templates/namespace.template.yaml"),
        Template::StacksApiConfigmap => {
            include_str!("../templates/stacks-api-configmap.template.yaml")
        }
        Template::StacksApiPod => include_str!("../templates/stacks-api-pod.template.yaml"),
        Template::StacksApiPostgresConfigmap => {
            include_str!("../templates/stacks-api-postgres-configmap.template.yaml")
        }
        Template::StacksApiPvc => include_str!("../templates/stacks-api-pvc.template.yaml"),
        Template::StacksApiService => include_str!("../templates/stacks-api-service.template.yaml"),
        Template::StacksNodeConfigmap => {
            include_str!("../templates/stacks-node-configmap.template.yaml")
        }
        Template::StacksNodePod => include_str!("../templates/stacks-node-pod.template.yaml"),
        Template::StacksNodeService => {
            include_str!("../templates/stacks-node-service.template.yaml")
        }
    }
}
