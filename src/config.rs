use clarinet_deployments::types::{DeploymentSpecification, TransactionSpecification};
use clarinet_files::{AccountConfig, DevnetConfig, FileLocation, NetworkManifest, ProjectManifest};
use hiro_system_kit::slog;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    resources::service::{get_service_port, ServicePort, StacksDevnetService},
    Context, DevNetError,
};

const PROJECT_ROOT: &str = "/etc/stacks-network/project";
const CONTRACT_DIR: &str = "/etc/stacks-network/project/contracts";
#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatedStacksDevnetConfig {
    pub namespace: String,
    pub user_id: String,
    pub devnet_config: DevnetConfig,
    pub accounts: BTreeMap<String, AccountConfig>,
    pub project_manifest_yaml_string: String,
    pub network_manifest_yaml_string: String,
    pub deployment_plan_yaml_string: String,
    pub contract_configmap_data: Vec<(String, String)>,
    pub disable_stacks_api: bool,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StacksDevnetConfig {
    pub namespace: String,
    pub disable_stacks_api: bool,
    disable_bitcoin_explorer: Option<bool>, // todo: currently unused
    disable_stacks_explorer: Option<bool>,  // todo: currently unused
    deployment_plan: DeploymentSpecification,
    pub network_manifest: NetworkManifest,
    project_manifest: ProjectManifest,
}
impl StacksDevnetConfig {
    pub fn to_validated_config(
        self,
        user_id: &str,
        ctx: &Context,
    ) -> Result<ValidatedStacksDevnetConfig, DevNetError> {
        let context = format!(
            "failed to validate config for NAMESPACE: {}",
            self.namespace
        );

        if user_id != self.namespace {
            let msg =
                format!("{context}, ERROR: devnet namespace must match authenticated user id");
            ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
            return Err(DevNetError {
                message: msg.into(),
                code: 400,
            });
        }

        let project_manifest_yaml_string = self
            .get_project_manifest_yaml_string()
            .map_err(|e| log_and_return_err(e, &context, &ctx))?;

        let (network_manifest_yaml_string, devnet_config) = self
            .get_network_manifest_string_and_devnet_config()
            .map_err(|e| log_and_return_err(e, &context, &ctx))?;

        let deployment_plan_yaml_string = self
            .get_deployment_plan_yaml_string()
            .map_err(|e| log_and_return_err(e, &context, &ctx))?;

        let mut contracts: Vec<(String, String)> = vec![];
        for (contract_identifier, (src, _)) in self.deployment_plan.contracts {
            contracts.push((contract_identifier.name.to_string(), src));
        }

        Ok(ValidatedStacksDevnetConfig {
            namespace: self.namespace,
            user_id: user_id.to_owned(),
            devnet_config: devnet_config.to_owned(),
            accounts: self.network_manifest.accounts,
            project_manifest_yaml_string: project_manifest_yaml_string.to_owned(),
            network_manifest_yaml_string,
            deployment_plan_yaml_string,
            contract_configmap_data: contracts,
            disable_stacks_api: self.disable_stacks_api,
        })
    }

    fn get_network_manifest_string_and_devnet_config(
        &self,
    ) -> Result<(String, DevnetConfig), String> {
        let network_config = &self.network_manifest;

        let devnet_config = match &self.network_manifest.devnet {
            Some(devnet_config) => Ok(devnet_config),
            None => Err("network manifest is missing required devnet config"),
        }?;
        let mut devnet_config = devnet_config.clone();
        devnet_config.orchestrator_ingestion_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Ingestion)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.orchestrator_control_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Control)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.bitcoin_node_p2p_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::P2P)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.bitcoin_node_rpc_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::RPC)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.stacks_node_p2p_port =
            get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::P2P)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.stacks_node_rpc_port =
            get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::RPC)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.stacks_api_port =
            get_service_port(StacksDevnetService::StacksBlockchainApi, ServicePort::API)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.stacks_api_events_port =
            get_service_port(StacksDevnetService::StacksBlockchainApi, ServicePort::Event)
                .unwrap()
                .parse::<u16>()
                .unwrap();
        devnet_config.postgres_port =
            get_service_port(StacksDevnetService::StacksBlockchainApi, ServicePort::DB)
                .unwrap()
                .parse::<u16>()
                .unwrap();

        let yaml_str = match serde_yaml::to_string(&network_config) {
            Ok(s) => Ok(s),
            Err(e) => Err(format!("failed to parse devnet config: {}", e)),
        }?;

        Ok((yaml_str, devnet_config))
    }

    fn get_project_manifest_yaml_string(&self) -> Result<String, String> {
        let mut project_manifest = self.project_manifest.clone();
        project_manifest.location = FileLocation::from_path(PathBuf::from(PROJECT_ROOT));
        project_manifest.project.cache_location =
            FileLocation::from_path(PathBuf::from(CONTRACT_DIR));
        serde_yaml::to_string(&project_manifest)
            .map_err(|e| format!("failed to parse project manifest: {}", e))
    }

    pub fn get_deployment_plan_yaml_string(&self) -> Result<String, String> {
        let deployment = self.deployment_plan.clone();
        let contracts_loc = FileLocation::from_path(PathBuf::from(CONTRACT_DIR));
        for b in deployment.plan.batches {
            for t in b.transactions {
                match t {
                    TransactionSpecification::ContractPublish(mut spec) => {
                        spec.location = contracts_loc.clone();
                    }
                    TransactionSpecification::RequirementPublish(mut spec) => {
                        spec.location = contracts_loc.clone();
                    },
                    TransactionSpecification::EmulatedContractCall(_) | TransactionSpecification::EmulatedContractPublish(_) => {
                        return Err(format!("devnet deployment plans do not support emulated-contract-calls or emulated-contract-publish types"))
                    }
                    _ => {}
                }
            }
        }
        serde_yaml::to_string(&self.deployment_plan)
            .map_err(|e| format!("failed to parse deployment plan config: {}", e))
    }
}

fn log_and_return_err(e: String, context: &str, ctx: &Context) -> DevNetError {
    let msg = format!("{context}, ERROR: {e}");
    ctx.try_log(|logger: &hiro_system_kit::Logger| slog::warn!(logger, "{}", msg));
    DevNetError {
        message: msg.into(),
        code: 400,
    }
}
#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{BufReader, Read},
        str::from_utf8,
    };

    use crate::Context;

    use super::StacksDevnetConfig;

    fn read_file(file_path: &str) -> Vec<u8> {
        let file = File::open(file_path)
            .unwrap_or_else(|e| panic!("unable to read file {}\n{:?}", file_path, e));
        let mut file_reader = BufReader::new(file);
        let mut file_buffer = vec![];
        file_reader
            .read_to_end(&mut file_buffer)
            .unwrap_or_else(|e| panic!("unable to read file {}\n{:?}", file_path, e));
        file_buffer
    }

    fn get_template_config(file_path: &str) -> StacksDevnetConfig {
        let file_buffer = read_file(file_path);

        let config_file: StacksDevnetConfig = match serde_json::from_slice(&file_buffer) {
            Ok(s) => s,
            Err(e) => {
                panic!("Config file malformatted {}", e.to_string());
            }
        };
        config_file
    }

    #[test]
    fn it_converts_config_to_yaml() {
        let template = get_template_config("src/tests/fixtures/stacks-devnet-config.json");
        let user_id = &template.namespace.clone();
        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context::empty();
        let validated_config = template
            .to_validated_config(user_id, &ctx)
            .unwrap_or_else(|e| panic!("config validation test failed: {}", e.message));

        let expected_project_manifest = read_file("src/tests/fixtures/project-manifest.yaml");
        let expected_project_mainfest = from_utf8(&expected_project_manifest).unwrap();

        let expected_network_mainfest = read_file("src/tests/fixtures/network-manifest.yaml");
        let expected_network_mainfest = from_utf8(&expected_network_mainfest).unwrap();

        let expected_deployment_plan = read_file("src/tests/fixtures/deployment-plan.yaml");
        let expected_deployment_plan = from_utf8(&expected_deployment_plan).unwrap();

        let expected_contract_source = read_file("src/tests/fixtures/contract-source.clar");
        let expected_contract_source = from_utf8(&expected_contract_source).unwrap();

        assert_eq!(
            expected_project_mainfest,
            validated_config.project_manifest_yaml_string
        );
        assert_eq!(
            expected_network_mainfest,
            validated_config.network_manifest_yaml_string
        );
        assert_eq!(
            expected_deployment_plan,
            validated_config.deployment_plan_yaml_string
        );
        assert_eq!(
            expected_contract_source,
            validated_config.contract_configmap_data[0].1
        );
    }

    #[test]
    #[should_panic]
    fn it_requires_devnet_config() {
        let mut template = get_template_config("src/tests/fixtures/stacks-devnet-config.json");
        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: None,
            tracer: false,
        };
        template.network_manifest.devnet = None;
        let user_id = template.clone().namespace;
        template
            .to_validated_config(&user_id, &ctx)
            .unwrap_or_else(|e| panic!("config validation test failed: {}", e.message));
    }

    #[test]
    fn it_rejects_config_with_namespace_user_id_mismatch() {
        let template = get_template_config("src/tests/fixtures/stacks-devnet-config.json");
        let namespace = template.namespace.clone();
        let user_id = "wrong";
        match template.to_validated_config(user_id, &Context::empty()) {
            Ok(_) => {
                panic!("config validation with non-matching user_id should have been rejected")
            }
            Err(e) => {
                assert_eq!(e.code, 400);
                assert_eq!(e.message, format!("failed to validate config for NAMESPACE: {}, ERROR: devnet namespace must match authenticated user id", namespace));
            }
        }
    }
}
