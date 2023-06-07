use k8s_openapi::{
    api::core::v1::{ConfigMap, Namespace, PersistentVolumeClaim, Pod, Service},
    NamespaceResourceScope,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

use kube::{
    api::{Api, DeleteParams, PostParams, ResourceExt},
    Client,
};
use std::thread::sleep;

mod template_parser;
use template_parser::{get_yaml_from_filename, Template};

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

const BITCOIND_CHAIN_COORDINATOR_SERVICE_NAME: &str = "bitcoind-chain-coordinator-service";
const STACKS_NODE_SERVICE_NAME: &str = "stacks-node-service";

const BITCOIND_P2P_PORT: &str = "18444";
const BITCOIND_RPC_PORT: &str = "18443";
const STACKS_NODE_P2P_PORT: &str = "20444";
const STACKS_NODE_RPC_PORT: &str = "20443";
const CHAIN_COORDINATOR_INGESTION_PORT: &str = "20445";

pub async fn deploy_devnet(config: StacksDevnetConfig) -> Result<(), Box<dyn std::error::Error>> {
    let namespace = &config.namespace;

    deploy_namespace(&namespace).await?;
    deploy_bitcoin_node_pod(&config).await?;

    sleep(Duration::from_secs(5));

    deploy_stacks_node_pod(&config).await?;

    if !config.disable_stacks_api {
        deploy_stacks_api_pod(&namespace).await?;
    }
    Ok(())
}

pub async fn delete_devnet(namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    let _ = delete_namespace(namespace).await;
    let _ = delete_resource::<Pod>(namespace, "bitcoind-chain-coordinator").await;
    let _ = delete_resource::<Pod>(namespace, "stacks-node").await;
    let _ = delete_resource::<Pod>(namespace, "stacks-api").await;
    let _ = delete_resource::<ConfigMap>(namespace, "bitcoind-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "stacks-node-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "stacks-api-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "stacks-api-postgres-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "deployment-plan-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "devnet-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "project-dir-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "namespace-conf").await;
    let _ = delete_resource::<ConfigMap>(namespace, "project-manifest-conf").await;
    let _ = delete_resource::<Service>(namespace, "bitcoind-chain-coordinator-service").await;
    let _ = delete_resource::<Service>(namespace, "stacks-node-service").await;
    let _ = delete_resource::<Service>(namespace, "stacks-api-service").await;
    let _ = delete_resource::<PersistentVolumeClaim>(namespace, "stacks-api-pvc").await;
    Ok(())
}

async fn deploy_namespace(namespace_str: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    let namespace_api: Api<Namespace> = kube::Api::all(client);

    let template_str = get_yaml_from_filename(Template::Namespace);
    let mut namespace: Namespace = serde_yaml::from_str(template_str)?;

    namespace.metadata.name = Some(namespace_str.to_owned());
    namespace.metadata.labels = Some(BTreeMap::from([("name".into(), namespace_str.to_owned())]));

    let post_params = PostParams::default();
    let created_namespace = namespace_api.create(&post_params, &namespace).await?;
    let name = created_namespace.name_any();
    assert_eq!(namespace.name_any(), name);
    println!("Created {}", name);
    Ok(())
}

async fn deploy_pod(template: Template, namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    let pods_api: Api<Pod> = Api::namespaced(client, &namespace);

    let template_str = get_yaml_from_filename(template);
    let mut pod: Pod = serde_yaml::from_str(template_str)?;
    pod.metadata.namespace = Some(namespace.to_owned());

    let pp = PostParams::default();
    let response = pods_api.create(&pp, &pod).await?;
    let name = response.name_any();
    println!("created pod {}", name);
    Ok(())
}

async fn deploy_service(
    template: Template,
    namespace: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    let service_api: Api<Service> = Api::namespaced(client, &namespace);

    let template_str = get_yaml_from_filename(template);
    let mut service: Service = serde_yaml::from_str(template_str)?;
    service.metadata.namespace = Some(namespace.to_owned());

    let pp = PostParams::default();
    let response = service_api.create(&pp, &service).await?;
    let name = response.name_any();
    println!("created service {}", name);
    Ok(())
}

async fn deploy_configmap(
    template: Template,
    namespace: &str,
    configmap_data: Option<Vec<(&str, &str)>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    let config_map_api: Api<ConfigMap> = kube::Api::<ConfigMap>::namespaced(client, &namespace);

    let template_str = get_yaml_from_filename(template);
    let mut configmap: ConfigMap = serde_yaml::from_str(template_str)?;

    configmap.metadata.namespace = Some(namespace.to_owned());
    if let Some(configmap_data) = configmap_data {
        let mut map = BTreeMap::new();
        for (key, value) in configmap_data {
            map.insert(key.into(), value.into());
        }
        configmap.data = Some(map);
    }

    let post_params = PostParams::default();
    let created_config = config_map_api.create(&post_params, &configmap).await?;
    let name = created_config.name_any();
    assert_eq!(configmap.name_any(), name);
    println!("Created {}", name);
    Ok(())
}

async fn deploy_bitcoin_node_pod(
    config: &StacksDevnetConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let bitcoind_p2p_port = BITCOIND_P2P_PORT.parse::<i32>()?;
    let bitcoind_rpc_port = BITCOIND_RPC_PORT.parse::<i32>()?;

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
        bitcoind_p2p_port,
        bitcoind_rpc_port,
        bitcoind_rpc_port
    );

    deploy_configmap(
        Template::BitcoindConfigmap,
        &namespace,
        Some(vec![("bitcoin.conf", &bitcoind_conf)]),
    )
    .await?;

    deploy_configmap(
        Template::ChainCoordinatorNamespaceConfigmap,
        &namespace,
        Some(vec![("NAMESPACE", &namespace)]),
    )
    .await?;

    deploy_configmap(
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
    println!("{}", devnet_config);
    deploy_configmap(
        Template::ChainCoordinatorDevnetConfigmap,
        &namespace,
        Some(vec![("Devnet.toml", &devnet_config)]),
    )
    .await?;

    deploy_configmap(
        Template::ChainCoordinatorDeploymentPlanConfigmap,
        &namespace,
        Some(vec![("default.devnet-plan.yaml", &config.deployment_plan)]),
    )
    .await?;

    let mut contracts: Vec<(&str, &str)> = vec![];
    for (contract_name, contract_source) in &config.contracts {
        contracts.push((contract_name, contract_source));
    }
    deploy_configmap(
        Template::ChainCoordinatorProjectDirConfigmap,
        &namespace,
        Some(contracts),
    )
    .await?;

    deploy_pod(Template::BitcoindChainCoordinatorPod, &namespace).await?;

    deploy_service(Template::BitcoindChainCoordinatorService, namespace).await?;

    Ok(())
}

async fn deploy_stacks_node_pod(
    config: &StacksDevnetConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let p2p_port = STACKS_NODE_P2P_PORT.parse::<i32>()?;
    let rpc_port = STACKS_NODE_RPC_PORT.parse::<i32>()?;
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
            rpc_port,
            p2p_port,
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

    deploy_configmap(
        Template::StacksNodeConfigmap,
        &namespace,
        Some(vec![("Stacks.toml", &stacks_conf)]),
    )
    .await?;

    deploy_pod(Template::StacksNodePod, &namespace).await?;

    deploy_service(Template::StacksNodeService, namespace).await?;

    Ok(())
}

async fn deploy_stacks_api_pod(namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    // configmap env vars for pg conatainer
    let stacks_api_pg_env = Vec::from([
        ("POSTGRES_PASSWORD", "postgres"),
        ("POSTGRES_DB", "stacks_api"),
    ]);
    deploy_configmap(
        Template::StacksApiPostgresConfigmap,
        &namespace,
        Some(stacks_api_pg_env),
    )
    .await?;

    // configmap env vars for api conatainer
    let namespaced_host = format!("{}.svc.cluster.local", &namespace);
    let stacks_node_host = format!("{}.{}", &STACKS_NODE_SERVICE_NAME, namespaced_host);
    let stacks_api_env = Vec::from([
        ("STACKS_CORE_RPC_HOST", &stacks_node_host[..]),
        ("STACKS_BLOCKCHAIN_API_DB", "pg"),
        ("STACKS_CORE_RPC_PORT", STACKS_NODE_RPC_PORT),
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
    deploy_configmap(
        Template::StacksApiConfigmap,
        &namespace,
        Some(stacks_api_env),
    )
    .await?;

    // deploy persistent volume claim
    {
        let client = Client::try_default().await?;
        let pvc_api: Api<PersistentVolumeClaim> = Api::namespaced(client, &namespace);

        let template_str = get_yaml_from_filename(Template::StacksApiPvc);
        let mut pvc: PersistentVolumeClaim = serde_yaml::from_str(template_str)?;
        pvc.metadata.namespace = Some(namespace.to_owned());

        let pp = PostParams::default();
        let response = pvc_api.create(&pp, &pvc).await?;
        let name = response.name_any();
        println!("created pod {}", name);
    }

    deploy_pod(Template::StacksApiPod, &namespace).await?;

    deploy_service(Template::StacksApiService, &namespace).await?;

    Ok(())
}

async fn delete_resource<K: kube::Resource<Scope = NamespaceResourceScope>>(
    namespace: &str,
    resource_name: &str,
) -> Result<(), Box<dyn std::error::Error>>
where
    <K as kube::Resource>::DynamicType: Default,
    K: Clone,
    K: DeserializeOwned,
    K: std::fmt::Debug,
{
    let client = Client::try_default().await?;
    let api: Api<K> = Api::namespaced(client, &namespace);
    let dp = DeleteParams::default();
    api.delete(resource_name, &dp).await?.map_left(|del| {
        assert_eq!(del.name_any(), resource_name);
        println!("Deleting resource started: {:?}", del);
    });
    Ok(())
}

async fn delete_namespace(namespace_str: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    let api: Api<Namespace> = kube::Api::all(client);

    let dp = DeleteParams::default();
    api.delete(namespace_str, &dp).await?.map_left(|del| {
        assert_eq!(del.name_any(), namespace_str);
        println!("Deleting resource started: {:?}", del);
    });
    Ok(())
}
