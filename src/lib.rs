use hyper::{body::Bytes, Body, Request, Response};
use k8s_openapi::{
    api::core::v1::{ConfigMap, Namespace, PersistentVolumeClaim, Pod, Service},
    NamespaceResourceScope,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeMap;
use tower::BoxError;

use kube::{
    api::{Api, DeleteParams, PostParams, ResourceExt},
    Client,
};

mod template_parser;
use template_parser::{get_yaml_from_filename, Template};

const BITCOIND_CHAIN_COORDINATOR_SERVICE_NAME: &str = "bitcoind-chain-coordinator-service";
const STACKS_NODE_SERVICE_NAME: &str = "stacks-node-service";

const BITCOIND_P2P_PORT: i32 = 18444;
const BITCOIND_RPC_PORT: i32 = 18443;
const STACKS_NODE_P2P_PORT: i32 = 20444;
const STACKS_NODE_RPC_PORT: i32 = 20443;
const CHAIN_COORDINATOR_INGESTION_PORT: i32 = 20445;
#[derive(Serialize, Deserialize, Debug)]
pub struct StacksDevnetConfig {
    namespace: String,
    label: String,
    network_id: u32,
    stacks_node_wait_time_for_microblocks: u32,
    stacks_node_first_attempt_time_ms: u32,
    stacks_node_subsequent_attempt_time_ms: u32,
    bitcoin_node_username: String,
    bitcoin_node_password: String,
    miner_mnemonic: String,
    miner_derivation_path: String,
    miner_coinbase_recipient: String,
    faucet_mnemonic: String,
    faucet_derivation_path: String,
    bitcoin_controller_block_time: u32,
    bitcoin_controller_automining_disabled: bool,
    disable_bitcoin_explorer: bool,
    disable_stacks_explorer: bool,
    disable_stacks_api: bool,
    epoch_2_0: u32,
    epoch_2_05: u32,
    epoch_2_1: u32,
    epoch_2_2: u32,
    pox_2_activation: u32,
    pox_2_unlock_height: u32,
    accounts: Vec<(String, u64)>,
    // needed for chain coordinator
    project_manifest: String,
    devnet_config: String,
    deployment_plan: String,
    contracts: Vec<(String, String)>,
    // to remove and compute
    stacks_miner_secret_key_hex: String,
    miner_stx_address: String,
}

pub struct DevNetError {
    pub message: String,
    pub code: u16,
}

#[derive(Clone)]
pub struct StacksDevnetApiK8sManager {
    client: Client,
}

impl StacksDevnetApiK8sManager {
    pub async fn default() -> StacksDevnetApiK8sManager {
        let client = Client::try_default()
            .await
            .expect("could not create kube client");
        StacksDevnetApiK8sManager { client }
    }

    pub async fn new<S, B, T>(service: S, default_namespace: T) -> StacksDevnetApiK8sManager
    where
        S: tower::Service<Request<Body>, Response = Response<B>> + Send + 'static,
        S::Future: Send + 'static,
        S::Error: Into<BoxError>,
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
        T: Into<String>,
    {
        let client = Client::new(service, default_namespace);
        StacksDevnetApiK8sManager { client }
    }

    pub async fn deploy_devnet(&self, config: StacksDevnetConfig) -> Result<(), DevNetError> {
        let namespace = &config.namespace;

        let namespace_exists = &self.check_namespace_exists(&namespace).await?;
        if !namespace_exists {
            if cfg!(debug_assertions) {
                self.deploy_namespace(&namespace).await?;
            } else {
                return Err(DevNetError {
                    message: "Cannot create devnet before namespace exists.".into(),
                    code: 400,
                });
            }
        }
        self.deploy_bitcoin_node_pod(&config).await?;

        self.deploy_stacks_node_pod(&config).await?;

        if !config.disable_stacks_api {
            self.deploy_stacks_api_pod(&namespace).await?;
        }
        Ok(())
    }

    pub async fn delete_devnet(&self, namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
        if cfg!(debug_assertions) {
            let _ = self.delete_namespace(namespace).await;
        }
        let _ = self
            .delete_resource::<Pod>(namespace, "bitcoind-chain-coordinator")
            .await;
        let _ = self.delete_resource::<Pod>(namespace, "stacks-node").await;
        let _ = self.delete_resource::<Pod>(namespace, "stacks-api").await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "bitcoind-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "stacks-node-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "stacks-api-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "stacks-api-postgres-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "deployment-plan-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "devnet-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "project-dir-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "namespace-conf")
            .await;
        let _ = self
            .delete_resource::<ConfigMap>(namespace, "project-manifest-conf")
            .await;
        let _ = self
            .delete_resource::<Service>(namespace, "bitcoind-chain-coordinator-service")
            .await;
        let _ = self
            .delete_resource::<Service>(namespace, "stacks-node-service")
            .await;
        let _ = self
            .delete_resource::<Service>(namespace, "stacks-api-service")
            .await;
        let _ = self
            .delete_resource::<PersistentVolumeClaim>(namespace, "stacks-api-pvc")
            .await;
        Ok(())
    }

    pub async fn check_namespace_exists(&self, namespace_str: &str) -> Result<bool, DevNetError> {
        let namespace_api: Api<Namespace> = kube::Api::all(self.client.to_owned());
        match namespace_api.get(namespace_str).await {
            Ok(_) => Ok(true),
            Err(kube::Error::Api(api_error)) => {
                if api_error.code == 404 {
                    Ok(false)
                } else {
                    Err(DevNetError {
                        message: format!("unable to get namespace: {}", api_error.message),
                        code: api_error.code,
                    })
                }
            }
            Err(e) => Err(DevNetError {
                message: format!("unable to get namespace: {}", e.to_string()),
                code: 500,
            }),
        }
    }

    async fn deploy_namespace(&self, namespace_str: &str) -> Result<(), DevNetError> {
        let mut namespace: Namespace = get_resource_from_file(Template::Namespace)?;

        namespace.metadata.name = Some(namespace_str.to_owned());
        namespace.metadata.labels =
            Some(BTreeMap::from([("name".into(), namespace_str.to_owned())]));

        let namespace_api: Api<Namespace> = kube::Api::all(self.client.to_owned());

        let pp = PostParams::default();
        match namespace_api.create(&pp, &namespace).await {
            Ok(namespace) => {
                let name = namespace.name_any();
                println!("created namespace {}", name);
                Ok(())
            }
            Err(kube::Error::Api(api_error)) => Err(DevNetError {
                message: format!("unable to create namespace: {}", api_error.message),
                code: api_error.code,
            }),
            Err(e) => Err(DevNetError {
                message: format!("unable to create namespace: {}", e.to_string()),
                code: 500,
            }),
        }
    }

    async fn deploy_resource<K: kube::Resource<Scope = NamespaceResourceScope>>(
        &self,
        namespace: &str,
        resource: K,
        resource_name: &str,
    ) -> Result<(), DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: Serialize,
    {
        let resource_api: Api<K> = Api::namespaced(self.client.to_owned(), &namespace);
        let pp = PostParams::default();

        match resource_api.create(&pp, &resource).await {
            Ok(resource) => {
                let name = resource.name_any();
                println!("created {} {}", resource_name, name);
                Ok(())
            }
            Err(kube::Error::Api(api_error)) => Err(DevNetError {
                message: format!("unable to create {}: {}", resource_name, api_error.message),
                code: api_error.code,
            }),
            Err(e) => Err(DevNetError {
                message: format!("unable to create {}: {}", resource_name, e.to_string()),
                code: 500,
            }),
        }
    }

    async fn deploy_pod(&self, template: Template, namespace: &str) -> Result<(), DevNetError> {
        let mut pod: Pod = get_resource_from_file(template)?;

        pod.metadata.namespace = Some(namespace.to_owned());
        self.deploy_resource(namespace, pod, "pod").await
    }

    async fn deploy_service(&self, template: Template, namespace: &str) -> Result<(), DevNetError> {
        let mut service: Service = get_resource_from_file(template)?;

        service.metadata.namespace = Some(namespace.to_owned());
        self.deploy_resource(namespace, service, "service").await
    }

    async fn deploy_configmap(
        &self,
        template: Template,
        namespace: &str,
        configmap_data: Option<Vec<(&str, &str)>>,
    ) -> Result<(), DevNetError> {
        let mut configmap: ConfigMap = get_resource_from_file(template)?;

        configmap.metadata.namespace = Some(namespace.to_owned());
        if let Some(configmap_data) = configmap_data {
            let mut map = BTreeMap::new();
            for (key, value) in configmap_data {
                map.insert(key.into(), value.into());
            }
            configmap.data = Some(map);
        }

        self.deploy_resource(namespace, configmap, "configmap")
            .await
    }

    async fn deploy_pvc(&self, template: Template, namespace: &str) -> Result<(), DevNetError> {
        let mut pvc: PersistentVolumeClaim = get_resource_from_file(template)?;

        pvc.metadata.namespace = Some(namespace.to_owned());

        self.deploy_resource(namespace, pvc, "pvc").await
    }

    async fn deploy_bitcoin_node_pod(
        &self,
        config: &StacksDevnetConfig,
    ) -> Result<(), DevNetError> {
        let namespace = &config.namespace;

        let bitcoind_conf = format!(
            r#"
                server=1
                regtest=1
                rpcallowip=0.0.0.0/0
                rpcallowip=::/0
                rpcuser={}
                rpcpassword={}
                txindex=1
                listen=1
                discover=0
                dns=0
                dnsseed=0
                listenonion=0
                rpcworkqueue=100
                rpcserialversion=1
                disablewallet=0
                fallbackfee=0.00001
                
                [regtest]
                bind=0.0.0.0:{}
                rpcbind=0.0.0.0:{}
                rpcport={}
                "#,
            config.bitcoin_node_username,
            config.bitcoin_node_password,
            BITCOIND_P2P_PORT,
            BITCOIND_RPC_PORT,
            BITCOIND_RPC_PORT
        );

        self.deploy_configmap(
            Template::BitcoindConfigmap,
            &namespace,
            Some(vec![("bitcoin.conf", &bitcoind_conf)]),
        )
        .await?;

        self.deploy_configmap(
            Template::ChainCoordinatorNamespaceConfigmap,
            &namespace,
            Some(vec![("NAMESPACE", &namespace)]),
        )
        .await?;

        self.deploy_configmap(
            Template::ChainCoordinatorProjectManifestConfigmap,
            &namespace,
            Some(vec![("Clarinet.toml", &config.project_manifest)]),
        )
        .await?;

        let mut devnet_config = config.devnet_config.clone();
        //devnet_config.push_str("\n[devnet]");
        devnet_config.push_str(&format!(
            "\nbitcoin_node_username = \"{}\"",
            &config.bitcoin_node_username
        ));
        devnet_config.push_str(&format!(
            "\nbitcoin_node_password = \"{}\"",
            &config.bitcoin_node_password
        ));

        self.deploy_configmap(
            Template::ChainCoordinatorDevnetConfigmap,
            &namespace,
            Some(vec![("Devnet.toml", &devnet_config)]),
        )
        .await?;

        self.deploy_configmap(
            Template::ChainCoordinatorDeploymentPlanConfigmap,
            &namespace,
            Some(vec![("default.devnet-plan.yaml", &config.deployment_plan)]),
        )
        .await?;

        let mut contracts: Vec<(&str, &str)> = vec![];
        for (contract_name, contract_source) in &config.contracts {
            contracts.push((contract_name, contract_source));
        }
        self.deploy_configmap(
            Template::ChainCoordinatorProjectDirConfigmap,
            &namespace,
            Some(contracts),
        )
        .await?;

        self.deploy_pod(Template::BitcoindChainCoordinatorPod, &namespace)
            .await?;

        self.deploy_service(Template::BitcoindChainCoordinatorService, namespace)
            .await?;

        Ok(())
    }

    async fn deploy_stacks_node_pod(&self, config: &StacksDevnetConfig) -> Result<(), DevNetError> {
        let namespace = &config.namespace;

        let stacks_conf = {
            let mut stacks_conf = format!(
                r#"
                    [node]
                    working_dir = "/devnet"
                    rpc_bind = "0.0.0.0:{}"
                    p2p_bind = "0.0.0.0:{}"
                    miner = true
                    seed = "{}"
                    local_peer_seed = "{}"
                    pox_sync_sample_secs = 0
                    wait_time_for_blocks = 0
                    wait_time_for_microblocks = {}
                    microblock_frequency = 1000

                    [connection_options]
                    # inv_sync_interval = 10
                    # download_interval = 10
                    # walk_interval = 10
                    disable_block_download = true
                    disable_inbound_handshakes = true
                    disable_inbound_walks = true
                    public_ip_address = "1.1.1.1:1234"

                    [miner]
                    first_attempt_time_ms = {}
                    subsequent_attempt_time_ms = {}
                    block_reward_recipient = "{}"
                    # microblock_attempt_time_ms = 15000
                "#,
                STACKS_NODE_RPC_PORT,
                STACKS_NODE_P2P_PORT,
                config.stacks_miner_secret_key_hex,
                config.stacks_miner_secret_key_hex,
                config.stacks_node_wait_time_for_microblocks,
                config.stacks_node_first_attempt_time_ms,
                config.stacks_node_subsequent_attempt_time_ms,
                config.miner_coinbase_recipient
            );

            for (address, balance) in config.accounts.iter() {
                stacks_conf.push_str(&format!(
                    r#"
                    [[ustx_balance]]
                    address = "{}"
                    amount = {}
                "#,
                    address, balance
                ));
            }

            let balance: u64 = 100_000_000_000_000;
            stacks_conf.push_str(&format!(
                r#"
                [[ustx_balance]]
                address = "{}"
                amount = {}
                "#,
                config.miner_coinbase_recipient, balance
            ));

            let namespaced_host = format!("{}.svc.cluster.local", &namespace);
            let bitcoind_chain_coordinator_host = format!(
                "{}.{}",
                &BITCOIND_CHAIN_COORDINATOR_SERVICE_NAME, namespaced_host
            );

            stacks_conf.push_str(&format!(
                r#"
                # Add orchestrator (docker-host) as an event observer
                [[events_observer]]
                endpoint = "{}:{}"
                retry_count = 255
                include_data_events = true
                events_keys = ["*"]
                "#,
                bitcoind_chain_coordinator_host, CHAIN_COORDINATOR_INGESTION_PORT
            ));

            //         stacks_conf.push_str(&format!(
            //             r#"
            // # Add stacks-api as an event observer
            // [[events_observer]]
            // endpoint = "host.docker.internal:{}"
            // retry_count = 255
            // include_data_events = false
            // events_keys = ["*"]
            // "#,
            //             30007,
            //         ));

            stacks_conf.push_str(&format!(
                r#"
                [burnchain]
                chain = "bitcoin"
                mode = "krypton"
                poll_time_secs = 1
                timeout = 30
                peer_host = "{}" 
                rpc_ssl = false
                wallet_name = "devnet"
                username = "{}"
                password = "{}"
                rpc_port = {}
                peer_port = {}
                "#,
                bitcoind_chain_coordinator_host,
                config.bitcoin_node_username,
                config.bitcoin_node_password,
                CHAIN_COORDINATOR_INGESTION_PORT,
                BITCOIND_P2P_PORT
            ));

            stacks_conf.push_str(&format!(
                r#"
                pox_2_activation = {}

                [[burnchain.epochs]]
                epoch_name = "1.0"
                start_height = 0

                [[burnchain.epochs]]
                epoch_name = "2.0"
                start_height = {}

                [[burnchain.epochs]]
                epoch_name = "2.05"
                start_height = {}

                [[burnchain.epochs]]
                epoch_name = "2.1"
                start_height = {}
                "#,
                config.pox_2_activation, config.epoch_2_0, config.epoch_2_05, config.epoch_2_1
            ));
            stacks_conf
        };

        self.deploy_configmap(
            Template::StacksNodeConfigmap,
            &namespace,
            Some(vec![("Stacks.toml", &stacks_conf)]),
        )
        .await?;

        self.deploy_pod(Template::StacksNodePod, &namespace).await?;

        self.deploy_service(Template::StacksNodeService, namespace)
            .await?;

        Ok(())
    }

    async fn deploy_stacks_api_pod(&self, namespace: &str) -> Result<(), DevNetError> {
        // configmap env vars for pg conatainer
        let stacks_api_pg_env = Vec::from([
            ("POSTGRES_PASSWORD", "postgres"),
            ("POSTGRES_DB", "stacks_api"),
        ]);
        self.deploy_configmap(
            Template::StacksApiPostgresConfigmap,
            &namespace,
            Some(stacks_api_pg_env),
        )
        .await?;

        // configmap env vars for api conatainer
        let namespaced_host = format!("{}.svc.cluster.local", &namespace);
        let stacks_node_host = format!("{}.{}", &STACKS_NODE_SERVICE_NAME, namespaced_host);
        let rpc_port = STACKS_NODE_RPC_PORT.to_string();
        let stacks_api_env = Vec::from([
            ("STACKS_CORE_RPC_HOST", &stacks_node_host[..]),
            ("STACKS_BLOCKCHAIN_API_DB", "pg"),
            ("STACKS_CORE_RPC_PORT", &rpc_port),
            ("STACKS_BLOCKCHAIN_API_PORT", "3999"),
            ("STACKS_BLOCKCHAIN_API_HOST", "0.0.0.0"),
            ("STACKS_CORE_EVENT_PORT", "3700"),
            ("STACKS_CORE_EVENT_HOST", "0.0.0.0"),
            ("STACKS_API_ENABLE_FT_METADATA", "1"),
            ("PG_HOST", "0.0.0.0"),
            ("PG_PORT", "5432"),
            ("PG_USER", "postgres"),
            ("PG_PASSWORD", "postgres"),
            ("PG_DATABASE", "stacks_api"),
            ("STACKS_CHAIN_ID", "2147483648"),
            ("V2_POX_MIN_AMOUNT_USTX", "90000000260"),
            ("NODE_ENV", "production"),
            ("STACKS_API_LOG_LEVEL", "debug"),
        ]);
        self.deploy_configmap(
            Template::StacksApiConfigmap,
            &namespace,
            Some(stacks_api_env),
        )
        .await?;

        self.deploy_pvc(Template::StacksApiPvc, &namespace).await?;

        self.deploy_pod(Template::StacksApiPod, &namespace).await?;

        self.deploy_service(Template::StacksApiService, &namespace)
            .await?;

        Ok(())
    }

    async fn delete_resource<K: kube::Resource<Scope = NamespaceResourceScope>>(
        &self,
        namespace: &str,
        resource_name: &str,
    ) -> Result<(), DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
    {
        let api: Api<K> = Api::namespaced(self.client.to_owned(), &namespace);
        let dp = DeleteParams::default();
        match api.delete(resource_name, &dp).await {
            Ok(resource) => {
                resource.map_left(|del| {
                    assert_eq!(del.name_any(), resource_name);
                    println!("Deleting {resource_name} started");
                });
                Ok(())
            }
            Err(kube::Error::Api(api_error)) => Err(DevNetError {
                message: format!("unable to delete {}: {}", resource_name, api_error.message),
                code: api_error.code,
            }),
            Err(e) => Err(DevNetError {
                message: format!("unable to delete {}: {}", resource_name, e.to_string()),
                code: 500,
            }),
        }
    }

    async fn delete_namespace(&self, namespace_str: &str) -> Result<(), DevNetError> {
        let api: Api<Namespace> = kube::Api::all(self.client.to_owned());

        let dp = DeleteParams::default();
        match api.delete(namespace_str, &dp).await {
            Ok(namespace) => {
                namespace.map_left(|del| {
                    assert_eq!(del.name_any(), namespace_str);
                    println!("Deleting namespace started");
                });
                Ok(())
            }
            Err(kube::Error::Api(api_error)) => Err(DevNetError {
                message: format!("unable to delete namespace: {}", api_error.message),
                code: api_error.code,
            }),
            Err(e) => Err(DevNetError {
                message: format!("unable to delete namespace: {}", e.to_string()),
                code: 500,
            }),
        }
    }
}

fn get_resource_from_file<K>(template: Template) -> Result<K, DevNetError>
where
    K: DeserializeOwned,
{
    let template_str = get_yaml_from_filename(template);

    let resource: K = serde_yaml::from_str(template_str).map_err(|e| DevNetError {
        message: format!("unable to parse template file: {}", e.to_string()),
        code: 500,
    })?;
    Ok(resource)
}
