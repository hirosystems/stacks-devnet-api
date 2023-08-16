use base64::{engine::general_purpose, Engine as _};
use clarinet_deployments::types::DeploymentSpecificationFile;
use clarinet_files::{
    DEFAULT_DERIVATION_PATH,
    DEFAULT_EPOCH_2_0,
    DEFAULT_EPOCH_2_05,
    DEFAULT_EPOCH_2_1,
    DEFAULT_FAUCET_MNEMONIC,
    DEFAULT_STACKS_MINER_MNEMONIC, //DEFAULT_EPOCH_2_2 (TODO, add when clarinet_files is updated)
};
use hiro_system_kit::slog;
use serde::{Deserialize, Serialize};
use std::str::from_utf8;

use crate::{
    resources::service::{get_service_port, ServicePort, StacksDevnetService},
    Context, DevNetError,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatedStacksDevnetConfig {
    pub user_config: StacksDevnetConfig,
    pub project_manifest_yaml_string: String,
    pub network_manifest_yaml_string: String,
    pub deployment_plan_yaml_string: String,
    pub contract_configmap_data: Vec<(String, String)>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct StacksDevnetConfig {
    pub namespace: String,
    pub stacks_node_wait_time_for_microblocks: Option<u32>,
    pub stacks_node_first_attempt_time_ms: Option<u32>,
    pub stacks_node_subsequent_attempt_time_ms: Option<u32>,
    pub bitcoin_node_username: String,
    pub bitcoin_node_password: String,
    pub miner_mnemonic: Option<String>,
    pub miner_derivation_path: Option<String>,
    pub miner_coinbase_recipient: Option<String>,
    faucet_mnemonic: Option<String>,
    faucet_derivation_path: Option<String>,
    bitcoin_controller_block_time: Option<u32>,
    bitcoin_controller_automining_disabled: Option<bool>,
    disable_bitcoin_explorer: Option<bool>, // todo: currently unused
    disable_stacks_explorer: Option<bool>,  // todo: currently unused
    pub disable_stacks_api: bool,
    pub epoch_2_0: Option<u64>,
    pub epoch_2_05: Option<u64>,
    pub epoch_2_1: Option<u64>,
    pub epoch_2_2: Option<u64>,
    pub pox_2_activation: Option<u64>,
    pub pox_2_unlock_height: Option<u32>, // todo (not currently used)
    project_manifest: ProjectManifestConfig,
    pub accounts: Vec<AccountConfig>,
    deployment_plan: DeploymentSpecificationFile,
    contracts: Vec<ContractConfig>,
}
impl StacksDevnetConfig {
    pub fn to_validated_config(
        self,
        user_id: &str,
        ctx: Context,
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
        let project_manifest_yaml_string = self.get_project_manifest_yaml_string();
        let network_manifest_yaml_string = self.get_network_manifest_yaml_string();
        let deployment_plan_yaml_string = match self.get_deployment_plan_yaml_string() {
            Ok(s) => Ok(s),
            Err(e) => {
                let msg = format!("{context}, ERROR: {e}");
                ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg.into(),
                    code: 400,
                })
            }
        }?;

        let mut contracts: Vec<(String, String)> = vec![];
        for contract in &self.contracts {
            let data = match contract.to_configmap_data() {
                Ok(d) => Ok(d),
                Err(e) => {
                    let msg = format!("{context}, ERROR: {e}");
                    ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
                    Err(DevNetError {
                        message: msg.into(),
                        code: 400,
                    })
                }
            }?;
            contracts.push(data);
        }
        Ok(ValidatedStacksDevnetConfig {
            user_config: self,
            project_manifest_yaml_string,
            network_manifest_yaml_string,
            deployment_plan_yaml_string,
            contract_configmap_data: contracts,
        })
    }

    fn get_project_manifest_yaml_string(&self) -> String {
        self.project_manifest.to_yaml_string(&self)
    }

    fn get_network_manifest_yaml_string(&self) -> String {
        let mut config = format!(
            r#"[network]
name = 'devnet'
"#,
        );

        config.push_str(
            &self
                .accounts
                .clone()
                .iter()
                .map(|a| a.to_yaml_string())
                .collect::<Vec<String>>()
                .join("\n"),
        );
        config.push_str(&format!(
            r#"
[devnet]
miner_mnemonic = "{}"
miner_derivation_path = "{}"
bitcoin_node_username = "{}"
bitcoin_node_password = "{}"
faucet_mnemonic = "{}"
faucet_derivation_path = "{}"
orchestrator_ingestion_port = {}
orchestrator_control_port = {}
bitcoin_node_rpc_port = {}
stacks_node_rpc_port = {}
stacks_api_port = {}
epoch_2_0 = {}
epoch_2_05 = {}
epoch_2_1 = {}
epoch_2_2 = {}
working_dir = "/devnet"
bitcoin_controller_block_time = {}
bitcoin_controller_automining_disabled = {}"#,
            &self
                .miner_mnemonic
                .clone()
                .unwrap_or(DEFAULT_STACKS_MINER_MNEMONIC.into()),
            &self
                .miner_derivation_path
                .clone()
                .unwrap_or(DEFAULT_DERIVATION_PATH.into()),
            &self.bitcoin_node_username,
            &self.bitcoin_node_password,
            &self
                .faucet_mnemonic
                .clone()
                .unwrap_or(DEFAULT_FAUCET_MNEMONIC.into()),
            &self
                .faucet_derivation_path
                .clone()
                .unwrap_or(DEFAULT_DERIVATION_PATH.into()),
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Ingestion).unwrap(),
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Control).unwrap(),
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::RPC).unwrap(),
            get_service_port(StacksDevnetService::StacksNode, ServicePort::RPC).unwrap(),
            get_service_port(StacksDevnetService::StacksApi, ServicePort::API).unwrap(),
            &self.epoch_2_0.unwrap_or(DEFAULT_EPOCH_2_0),
            &self.epoch_2_05.unwrap_or(DEFAULT_EPOCH_2_05),
            &self.epoch_2_1.unwrap_or(DEFAULT_EPOCH_2_1),
            &self.epoch_2_2.unwrap_or(122), // todo: should be DEFAULT_EPOCH_2_2 when clarinet_files is updated
            &self.bitcoin_controller_block_time.unwrap_or(50),
            &self.bitcoin_controller_automining_disabled.unwrap_or(false)
        ));

        config
    }

    pub fn get_deployment_plan_yaml_string(&self) -> Result<String, String> {
        serde_yaml::to_string(&self.deployment_plan)
            .map_err(|e| format!("failed to parse deployment plan config: {}", e))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ProjectManifestConfig {
    name: String,
    description: Option<String>,
    authors: Option<Vec<String>>,
    requirements: Option<Vec<String>>,
}

impl ProjectManifestConfig {
    fn to_yaml_string(&self, config: &StacksDevnetConfig) -> String {
        let description = match &self.description {
            Some(d) => d.to_owned(),
            None => String::new(),
        };
        let authors = match &self.authors {
            Some(a) => format!("['{}']", a.join("','")),
            None => String::from("[]"),
        };
        let requirements = match &self.requirements {
            Some(r) => format!("['{}']", r.join("','")),
            None => String::from("[]"),
        };
        format!(
            r#"[project]
name = "{}"
description = "{}"
authors = {}
requirements = {}

{}"#,
            &self.name,
            description,
            authors,
            requirements,
            &config
                .contracts
                .clone()
                .into_iter()
                .map(|c| c.to_project_manifest_yaml_string())
                .collect::<Vec<String>>()
                .join("\n")
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContractConfig {
    pub name: String,
    pub source: String,
    clarity_version: u32,
    epoch: f64,
    deployer: Option<String>,
}

impl ContractConfig {
    fn to_project_manifest_yaml_string(&self) -> String {
        let mut config = format!(
            r#"[contracts.{}]
path = "contracts/{}.clar"
clarity_version = {}
epoch = "{}""#,
            &self.name, &self.name, self.clarity_version, self.epoch,
        );
        if let Some(deployer) = &self.deployer {
            config.push_str(&format!(r#"deployer = {}"#, deployer,));
        }
        config
    }

    fn to_configmap_data(&self) -> Result<(String, String), String> {
        let bytes = general_purpose::STANDARD
            .decode(&self.source)
            .map_err(|e| format!("unable to decode contract source: {}", e.to_string()))?;

        let decoded = from_utf8(&bytes).map_err(|e| {
            format!(
                "invalid UTF-8 sequence when decoding contract source: {}",
                e.to_string()
            )
        })?;
        let filename = format!("{}.clar", &self.name);
        Ok((filename, decoded.to_owned()))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountConfig {
    pub name: String,
    pub mnemonic: String,
    pub derivation: Option<String>,
    pub balance: u64,
}
impl AccountConfig {
    pub fn to_yaml_string(&self) -> String {
        let mut config = format!(
            r#"
[accounts.{}]
mnemonic = "{}"
balance = "{}"
"#,
            &self.name, &self.mnemonic, &self.balance
        );
        if let Some(derivation) = &self.derivation {
            config.push_str(&format!(r#"derivation = "{}""#, derivation));
        }
        config
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

    use super::{ProjectManifestConfig, StacksDevnetConfig};

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
    #[should_panic]
    fn it_rejects_config_with_none_base64_source_code() {
        let mut template = get_template_config("src/tests/fixtures/stacks-devnet-config.json");
        let user_id = &template.namespace.clone();
        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: None,
            tracer: false,
        };
        template.contracts[0].source = "invalid base64 string".to_string();
        template
            .to_validated_config(user_id, ctx)
            .unwrap_or_else(|e| panic!("config validation test failed: {}", e.message));
    }

    #[test]
    fn it_converts_config_to_yaml() {
        let template = get_template_config("src/tests/fixtures/stacks-devnet-config.json");
        let user_id = &template.namespace.clone();
        let logger = hiro_system_kit::log::setup_logger();
        let _guard = hiro_system_kit::log::setup_global_logger(logger.clone());
        let ctx = Context {
            logger: None,
            tracer: false,
        };
        let validated_config = template
            .to_validated_config(user_id, ctx)
            .unwrap_or_else(|e| panic!("config validation test failed: {}", e.message));

        let expected_project_manifest = read_file("src/tests/fixtures/project-manifest.yaml");
        let expected_project_mainfest = from_utf8(&expected_project_manifest).unwrap();

        let expected_network_mainfest = read_file("src/tests/fixtures/network-manifest.yaml");
        let expected_network_mainfest = from_utf8(&expected_network_mainfest).unwrap();

        let expected_deployment_plan = read_file("src/tests/fixtures/deployment-plan.yaml");
        let expected_deployment_plan = from_utf8(&expected_deployment_plan).unwrap();

        let expected_contract_source = read_file("src/tests/fixtures/contract-source.clar");
        let escaped = expected_contract_source
            .iter()
            .flat_map(|b| std::ascii::escape_default(*b))
            .collect::<Vec<u8>>();
        let expected_contract_source = from_utf8(&escaped).unwrap();

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
    fn project_manifest_allows_omitted_values() {
        let project_manifest = ProjectManifestConfig {
            name: "Test".to_string(),
            description: None,
            authors: None,
            requirements: None,
        };
        let mut template = get_template_config("src/tests/fixtures/stacks-devnet-config.json");
        template.contracts = vec![];
        let yaml = project_manifest.to_yaml_string(&template);
        let expected = r#"[project]
name = "Test"
description = ""
authors = []
requirements = []

"#;
        assert_eq!(expected.to_string(), yaml);
    }

    #[test]
    fn it_rejects_config_with_namespace_user_id_mismatch() {
        let template = get_template_config("src/tests/fixtures/stacks-devnet-config.json");
        let namespace = template.namespace.clone();
        let user_id = "wrong";
        match template.to_validated_config(user_id, Context::empty()) {
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
