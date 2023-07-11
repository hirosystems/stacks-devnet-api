use std::fmt;

use clarinet_deployments::types::{DeploymentSpecificationFile, EpochSpec};
use clarinet_files::{
    DEFAULT_DERIVATION_PATH,
    DEFAULT_EPOCH_2_0,
    DEFAULT_EPOCH_2_05,
    DEFAULT_EPOCH_2_1,
    DEFAULT_FAUCET_MNEMONIC,
    DEFAULT_STACKS_MINER_MNEMONIC, //DEFAULT_EPOCH_2_2 (TODO, add when clarinet_files is updated)
};
use serde::{Deserialize, Serialize};

use crate::resources::service::{get_service_port, ServicePort, StacksDevnetService};

#[derive(Serialize, Deserialize, Debug)]
pub struct StacksDevnetConfig {
    pub namespace: String,
    pub stacks_node_wait_time_for_microblocks: Option<u32>,
    pub stacks_node_first_attempt_time_ms: Option<u32>,
    pub stacks_node_subsequent_attempt_time_ms: Option<u32>,
    pub bitcoin_node_username: String,
    pub bitcoin_node_password: String,
    pub miner_mnemonic: Option<String>, // todo: should we remove these and just get them from the `accounts` field?
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
    deployment_fee_rate: Option<u64>,
    project_manifest: ProjectManifestConfig,
    pub accounts: Vec<AccountConfig>,
    deployment_plan: DeploymentSpecificationFile,
    pub contracts: Vec<ContractConfig>,
}
impl StacksDevnetConfig {
    pub fn get_project_manifest_yaml_string(&self) -> String {
        self.project_manifest.to_yaml_string(&self)
    }

    pub fn get_network_manifest_yaml_string(&self) -> String {
        let mut config = format!(
            r#"
                [network]
                name = 'devnet'
            "#,
        );

        if let Some(deployment_fee_rate) = &self.deployment_fee_rate {
            config.push_str(&format!(
                r#"
                    deployment_fee_rate = {}
                "#,
                deployment_fee_rate
            ))
        }

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
                bitcoin_controller_block_time = "{}"
                bitcoin_controller_automining_disabled = "{}"
            "#,
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

    pub fn get_deployment_plan_yaml_string(&self) -> String {
        serde_yaml::to_string(&self.deployment_plan).unwrap()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectManifestConfig {
    name: String,
    description: Option<String>,
    authors: Option<Vec<String>>,
    requirements: Option<Vec<String>>,
}

impl ProjectManifestConfig {
    pub fn to_yaml_string(&self, config: &StacksDevnetConfig) -> String {
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
            r#"
                [project]
                name = "{}"
                description = "{}"
                authors = {}
                requirements = {}

                {}
            "#,
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
enum ClarityVersion {
    Clarity1,
    Clarity2,
}
impl fmt::Display for ClarityVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClarityVersion::Clarity1 => write!(f, "1"),
            ClarityVersion::Clarity2 => write!(f, "2"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContractConfig {
    pub name: String,
    pub source: String,
    clarity_version: ClarityVersion,
    epoch: EpochSpec,
    deployer: Option<String>, // todo: can this just be derived from the NetworkManifest Accounts?
}

impl ContractConfig {
    pub fn to_project_manifest_yaml_string(&self) -> String {
        let mut config = format!(
            r#"
                [contracts.{}]
                path = "contracts/{}.clar"
                clarity_version = {}
                epoch = {:?}
            "#,
            &self.name, &self.name, self.clarity_version, self.epoch,
        );
        if let Some(deployer) = &self.deployer {
            config.push_str(&format!(
                r#"
                    deployer = {}
                "#,
                deployer,
            ));
        }
        config
    }

    pub fn to_configmap_data(&self) -> (String, &str) {
        let decoded = &self.source;
        let filename = format!("{}.clar", &self.name);
        (filename, decoded)
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
            config.push_str(&format!(
                r#"
                    derivation = "{}"
                "#,
                derivation
            ));
        }
        config
    }
}
