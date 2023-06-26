use std::collections::BTreeMap;

use clarinet_files::PoxStackingOrder;
use clarity::types::StacksEpochId;
use clarity_repl::repl::ContractDeployer;
use serde::{Deserialize, Serialize};

use crate::resources::service::{
    get_service_port, get_service_url, ServicePort, StacksDevnetService,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct StacksDevnetConfig {
    pub namespace: String,
    pub stacks_node_wait_time_for_microblocks: u32,
    pub stacks_node_first_attempt_time_ms: u32,
    pub stacks_node_subsequent_attempt_time_ms: u32,
    pub bitcoin_node_username: String,
    pub bitcoin_node_password: String,
    pub miner_mnemonic: String, // todo: should we remove these and just get them from the network manifest's accounts?
    pub miner_derivation_path: String,
    pub miner_coinbase_recipient: String,
    pub faucet_mnemonic: String,
    pub faucet_derivation_path: String,
    pub bitcoin_controller_block_time: u32,
    pub bitcoin_controller_automining_disabled: bool,
    pub disable_bitcoin_explorer: bool,
    pub disable_stacks_explorer: bool,
    pub disable_stacks_api: bool,
    pub epoch_2_0: u32,
    pub epoch_2_05: u32,
    pub epoch_2_1: u32,
    pub epoch_2_2: u32,
    pub pox_2_activation: u32,
    pub pox_2_unlock_height: u32,
    pub project_manifest: ProjectManifestConfig,
    pub network_manifest: NetworkManifestConfig,
    pub deployment_plan: String,
    pub contracts: Vec<(String, String)>,
}

impl StacksDevnetConfig {
    pub fn get_project_manifest_yaml_string(&self) -> String {
        self.project_manifest.to_yaml_string()
    }

    pub fn get_network_manifest_yaml_string(&self) -> String {
        self.network_manifest.to_yaml_string(&self)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectManifestConfig {
    name: String,
    description: Option<String>,
    authors: Option<Vec<String>>,
    requirements: Option<Vec<String>>,
    contracts: Vec<ProjectManifestContractConfig>,
}

impl ProjectManifestConfig {
    pub fn to_yaml_string(&self) -> String {
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
            &self
                .contracts
                .clone()
                .into_iter()
                .map(|c| c.to_yaml_string())
                .collect::<Vec<String>>()
                .join("\n")
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectManifestContractConfig {
    name: String,
    clarity_version: u8, // todo: fix type
    epoch: StacksEpochId,
    deployer: Option<ContractDeployer>, // todo: add to `to_yaml_str`. also, can this just be derived from the NetworkManifest Accounts?
}

impl ProjectManifestContractConfig {
    pub fn to_yaml_string(&self) -> String {
        format!(
            r#"
                [contracts.{}]
                path = "contracts/{}.clar"
                clarity_version = {}
                epoch = {}

            "#,
            &self.name, &self.name, self.clarity_version, self.epoch,
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NetworkManifestConfig {
    pub accounts: BTreeMap<String, AccountConfig>,
    devnet: DevnetConfig,
}
impl NetworkManifestConfig {
    pub fn to_yaml_string(&self, stacks_devnet_config: &StacksDevnetConfig) -> String {
        let namespace = &stacks_devnet_config.namespace;
        let mut config = format!(
            r#"
                [network]
                name = 'devnet'
                stacks_node_rpc_address = "{}" # todo: confirm if we need this field
                bitcoin_node_rpc_address = "{}" # todo: confirm if we need this field

            "#,
            format!(
                "http://{}:{}",
                get_service_url(namespace, StacksDevnetService::StacksNode),
                get_service_port(StacksDevnetService::StacksNode, ServicePort::RPC).unwrap()
            ),
            format!(
                "http://{}:{}",
                get_service_url(namespace, StacksDevnetService::BitcoindNode),
                get_service_port(StacksDevnetService::BitcoindNode, ServicePort::RPC).unwrap()
            )
        );

        config.push_str(
            &self
                .accounts
                .clone()
                .iter()
                .map(|(name, account)| account.to_yaml_string(name))
                .collect::<Vec<String>>()
                .join("\n"),
        );

        config.push_str(&self.devnet.to_yaml_string(stacks_devnet_config));
        config
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
// todo: see if any of these fields are needed
pub struct DevnetConfig {
    pub pox_stacking_orders: Option<Vec<PoxStackingOrder>>,
    pub enable_subnet_node: Option<bool>,
    pub subnet_leader_stx_address: Option<String>,
    pub subnet_contract_id: Option<String>,
}
impl DevnetConfig {
    pub fn to_yaml_string(&self, config: &StacksDevnetConfig) -> String {
        format!(
            r#"
                [devnet]
                miner_mnemonic = "{}"
                miner_derivation_path = "{}"
                faucet_mnemonic = "{}"
                faucet_derivation_path = "{}"
                bitcoin_node_username = "{}"
                bitcoin_node_password = "{}"
                orchestrator_ingestion_port = {}
                orchestrator_control_port = {}
                bitcoin_node_rpc_port = {}
                stacks_node_rpc_port = {}
                stacks_api_port = {}
                bitcoin_controller_block_time = {}
                bitcoin_controller_automining_disabled = {}
                epoch_2_0 = {}
                epoch_2_05 = {}
                epoch_2_1 = {}
                epoch_2_2 = {}
                working_dir = "/devnet"
            "#,
            config.miner_mnemonic,
            config.miner_derivation_path,
            config.faucet_mnemonic,
            config.faucet_derivation_path,
            config.bitcoin_node_username,
            config.bitcoin_node_password,
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Ingestion).unwrap(),
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Control).unwrap(),
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::RPC).unwrap(),
            get_service_port(StacksDevnetService::StacksNode, ServicePort::RPC).unwrap(),
            get_service_port(StacksDevnetService::StacksApi, ServicePort::API).unwrap(),
            config.bitcoin_controller_block_time,
            config.bitcoin_controller_automining_disabled,
            config.epoch_2_0,
            config.epoch_2_05,
            config.epoch_2_1,
            config.epoch_2_2,
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountConfig {
    pub mnemonic: String,
    //pub derivation: String,
    pub balance: u64,
    pub stx_address: String,
    //pub btc_address: String,
}
impl AccountConfig {
    pub fn to_yaml_string(&self, name: &String) -> String {
        format!(
            r#"
                [accounts.{}]
                mnemonic = "{}"
                balance = "{}"
            "#,
            name, self.mnemonic, self.balance
        )
    }
}
