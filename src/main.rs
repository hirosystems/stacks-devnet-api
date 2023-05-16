// use std::{path::PathBuf, str::FromStr};

use std::{collections::BTreeMap, time::Duration};

// use k8s_experimentation::Thing;
use k8s_openapi::{
    api::core::v1::{
        ConfigMap, ConfigMapEnvSource, ConfigMapVolumeSource, Container, ContainerPort,
        EnvFromSource, EnvVar, HostPathVolumeSource, PersistentVolumeClaim,
        PersistentVolumeClaimSpec, PersistentVolumeClaimVolumeSource, Pod, ResourceRequirements,
        Service, ServicePort, Volume, VolumeMount,
    },
    apimachinery::pkg::api::resource::Quantity,
};
use serde_json::json;

use kube::{
    api::{Api, ListParams, PostParams, ResourceExt},
    runtime::wait::{await_condition, conditions::is_pod_running},
    Client,
};
use std::thread::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    const BITCOIN_NODE_SERVICE_NAME: &str = "bitcoin-node-service";
    const ORCHESTRATOR_SERVICE_NAME: &str = "orchestrator-service";

    // values from user config
    // let user_id = "user-id";
    // let user_project_name = "my-project";
    let namespace = "px-devnet"; //format!("{user_id}-{user_project_name}-devnet");
    let bitcoin_node_image = "quay.io/hirosystems/bitcoind:devnet-v3";
    let bitcoin_node_p2p_port = "18444";
    let bitcoin_node_rpc_port = "18443";
    let stacks_node_image = "quay.io/hirosystems/stacks-node:devnet-v3";
    let stacks_node_p2p_port = "20444";
    let stacks_node_rpc_port = "20443";
    let stacks_miner_secret_key_hex =
        "7287ba251d44a4d3fd9276c88ce34c5c52a038955511cccaf77e61068649c17801";
    let miner_stx_address = "ST1SJ3DTE5DN7X54YDH5D64R3BCB6A2AG2ZQ8YPD5";
    let stacks_node_wait_time_for_microblocks: u32 = 50;
    let stacks_node_first_attempt_time_ms: u32 = 500;
    let stacks_node_subsequent_attempt_time_ms: u32 = 1_000;
    let orchestrator_ingestion_port = "20445";

    let stacks_api_image = "hirosystems/stacks-blockchain-api";
    let chain_coordinator_image = "stacks-network";

    deploy_bitcoin_node_pod(
        &namespace,
        &bitcoin_node_image,
        &bitcoin_node_p2p_port,
        &bitcoin_node_rpc_port,
        BITCOIN_NODE_SERVICE_NAME,
    )
    .await?;

    deploy_chain_coordinator(
        &namespace,
        &chain_coordinator_image,
        ORCHESTRATOR_SERVICE_NAME,
    )
    .await?;

    sleep(Duration::from_secs(10));

    deploy_stacks_node_pod(
        &namespace,
        &stacks_node_image,
        &stacks_node_p2p_port,
        &stacks_node_rpc_port,
        &stacks_miner_secret_key_hex,
        &stacks_node_wait_time_for_microblocks,
        &stacks_node_first_attempt_time_ms,
        &stacks_node_subsequent_attempt_time_ms,
        &miner_stx_address,
        // &bitcoin_node_p2p_port,
        // &bitcoin_node_rpc_port,
        // &orchestrator_ingestion_port,
        // BITCOIN_NODE_SERVICE_NAME,
        // ORCHESTRATOR_SERVICE_NAME,
    )
    .await?;

    deploy_stacks_api_pod(&namespace, &stacks_api_image).await?;
    let client = Client::try_default().await?;
    let pods_api: Api<Pod> = Api::namespaced(client, &namespace);
    // Watch it phase for a few seconds
    let establish = await_condition(pods_api.clone(), "bitcoin-node", is_pod_running());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(15), establish).await?;

    // Verify we can get it
    println!("Get Pod bitcoin-node");
    let p1cpy = pods_api.get("bitcoin-node").await?;
    if let Some(spec) = &p1cpy.spec {
        println!(
            "Got bitcoin-node pod with containers: {:?}",
            spec.containers
        );
    }

    let lp = ListParams::default().fields(&format!("metadata.name={}", "bitcoin-node")); // only want results for our pod
    for p in pods_api.list(&lp).await? {
        println!("Found Pod: {}", p.name_any());
    }

    // Delete it
    // let dp = DeleteParams::default();
    // pods.delete("blog", &dp).await?.map_left(|pdel| {
    //     assert_eq!(pdel.name_any(), "blog");
    //     println!("Deleting blog pod started: {:?}", pdel);
    // });

    Ok(())
}

async fn deploy_bitcoin_node_pod(
    namespace: &str,
    image: &str,
    p2p_port: &str,
    rpc_port: &str,
    service_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // constants for bitcoin pod, services, and config
    const POD_NAME: &str = "bitcoin-node";
    const CONTAINER_NAME: &str = "bitcoin-node-container";
    const CONFIGMAP_NAME: &str = "bitcoin-conf";
    const CONFIGMAP_VOLUME_NAME: &str = "bitcoin-conf-volume";

    // massage user data
    let p2p_port = p2p_port.parse::<i32>()?;
    let rpc_port = rpc_port.parse::<i32>()?;

    // deploy config map for bitcoin node
    {
        let client = Client::try_default().await?;
        let config_map_api: Api<ConfigMap> = kube::Api::<ConfigMap>::namespaced(client, &namespace);

        let bitcoind_conf = format!(
            r#"
                server=1
                regtest=1
                rpcallowip=0.0.0.0/0
                rpcallowip=::/0
                rpcuser={namespace}
                rpcpassword={namespace}
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
                bind=0.0.0.0:{p2p_port}
                rpcbind=0.0.0.0:{rpc_port}
                rpcport={rpc_port}
                "#
        );
        let config_map: ConfigMap = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": CONFIGMAP_NAME,
                "namespace": namespace
            },
            "data": {
                "bitcoin.conf": bitcoind_conf

            },
        }))?;

        let post_params = PostParams::default();
        let created_config = config_map_api.create(&post_params, &config_map).await?;
        let name = created_config.name_any();
        assert_eq!(config_map.name_any(), name);
        println!("Created {}", name);
    }

    // deploy bitcoin node pod
    {
        let client = Client::try_default().await?;
        let pods_api: Api<Pod> = Api::namespaced(client, &namespace);

        let bitcoin_pod: Pod = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": POD_NAME,
            "namespace": namespace,
            "labels": {"name": POD_NAME}
        },
        "spec": {
            "containers": Some(vec![ Container {
                name: CONTAINER_NAME.into(),
                image: Some(image.into()),
                command: Some(vec![
                    "/usr/local/bin/bitcoind".into(),
                    "-conf=/etc/bitcoin/bitcoin.conf".into(),
                    "-nodebuglogfile".into(),
                    "-pid=/run/bitcoind.pid".into(),
                ]),
                ports: Some(vec![
                    ContainerPort {
                        container_port: p2p_port,
                        protocol: Some("TCP".into()),
                        name: Some("p2p".into()),
                        ..Default::default()
                    },
                    ContainerPort {
                        container_port: rpc_port,
                        protocol: Some("TCP".into()),
                        name: Some("rpc".into()),
                        ..Default::default()
                    },
                    ContainerPort {
                        container_port: 20445,
                        protocol: Some("TCP".into()),
                        name: Some("orchestrator".into()),
                        ..Default::default()
                    },
                ]),
                volume_mounts: Some(vec![ VolumeMount {
                    name: CONFIGMAP_VOLUME_NAME.into(),
                    mount_path: "/etc/bitcoin".into(),
                    read_only: Some(true),
                    ..Default::default()
                }]),
                ..Default::default()
            }]),
            "volumes": Some(vec![
                Volume {
                name: CONFIGMAP_VOLUME_NAME.into(),
                config_map: Some(ConfigMapVolumeSource {
                    name: Some(CONFIGMAP_NAME.into())
                    , ..Default::default()
                }),
                ..Default::default()
            }])
        }}))?;

        let pp = PostParams::default();
        let response = pods_api.create(&pp, &bitcoin_pod).await?;
        let name = response.name_any();
        println!("created pod {}", name);
    }

    // deploy service to communicate with container
    {
        let client = Client::try_default().await?;
        let service_api: Api<Service> = Api::namespaced(client, &namespace);

        let mut selector = BTreeMap::<String, String>::new();
        selector.insert("name".into(), CONTAINER_NAME.into());

        let service: Service = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": service_name,
                "namespace": namespace
            },
            "spec":  {
                "type": "NodePort",
                "ports": Some(vec![ServicePort {
                    port: p2p_port,
                    protocol: Some("TCP".into()),
                    name: Some("p2p".into()),
                    node_port: Some(30000),
                    ..Default::default()
                },ServicePort {
                    port: rpc_port,
                    protocol: Some("TCP".into()),
                    name: Some("rpc".into()),
                    node_port: Some(30001),
                    ..Default::default()
                }]),
                "selector":  {"name": POD_NAME},
            }
        }))?;

        let pp = PostParams::default();
        let response = service_api.create(&pp, &service).await?;
        let name = response.name_any();
        println!("created service {}", name);
    }
    Ok(())
}

async fn deploy_stacks_node_pod(
    namespace: &str,
    image: &str,
    p2p_port: &str,
    rpc_port: &str,
    miner_secret_key_hex: &str,
    wait_time_for_microblocks: &u32,
    first_attempt_time_ms: &u32,
    subsequent_attempt_time_ms: &u32,
    miner_coinbase_recipient: &str,
    // bitcoin_node_p2p_port: &str,
    // bitcoin_node_rpc_port: &str,
    // orchestrator_ingestion_port: &str,
    // bitcoin_node_service_name: &str,
    // orchestrator_service_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // constants for stacks pod, services, and config
    const POD_NAME: &str = "stacks-node";
    const CONTAINER_NAME: &str = "stacks-node-container";
    const CONFIGMAP_NAME: &str = "stacks-conf";
    const CONFIGMAP_VOLUME_NAME: &str = "stacks-conf-volume";
    const SERVICE_NAME: &str = "stacks-node-service";

    // massage user data
    let p2p_port = p2p_port.parse::<i32>()?;
    let rpc_port = rpc_port.parse::<i32>()?;

    // deploy config map for stacks node
    {
        let client = Client::try_default().await?;
        let config_map_api: Api<ConfigMap> = kube::Api::<ConfigMap>::namespaced(client, &namespace);

        let mut stacks_conf = format!(
            r#"
                [node]
                working_dir = "/devnet"
                rpc_bind = "0.0.0.0:{rpc_port}"
                p2p_bind = "0.0.0.0:{p2p_port}"
                miner = true
                seed = "{miner_secret_key_hex}"
                local_peer_seed = "{miner_secret_key_hex}"
                pox_sync_sample_secs = 0
                wait_time_for_blocks = 0
                wait_time_for_microblocks = {wait_time_for_microblocks}
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
                first_attempt_time_ms = {first_attempt_time_ms}
                subsequent_attempt_time_ms = {subsequent_attempt_time_ms}
                block_reward_recipient = "{miner_coinbase_recipient}"
                # microblock_attempt_time_ms = 15000
                "#
        );

        let balance: u64 = 100_000_000_000_000;
        stacks_conf.push_str(&format!(
            r#"
        [[ustx_balance]]
        address = "{miner_coinbase_recipient}"
        amount = {balance}
        "#
        ));

        let cluster_domain = "cluster.local";

        stacks_conf.push_str(&format!(
            r#"
# Add orchestrator (docker-host) as an event observer
[[events_observer]]
endpoint = "host.docker.internal:30008"
retry_count = 255
include_data_events = true
events_keys = ["*"]
"#
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
peer_host = "host.docker.internal" 
rpc_ssl = false
wallet_name = "devnet"
username = "{namespace}"
password = "{namespace}"
rpc_port = {}
peer_port = {}
"#,
            30008, 30000
        ));

        let pox_2_activation = 112;
        let epoch_2_0 = 100;
        let epoch_2_05 = 102;
        let epoch_2_1 = 106;
        stacks_conf.push_str(&format!(
            r#"pox_2_activation = {pox_2_activation}

[[burnchain.epochs]]
epoch_name = "1.0"
start_height = 0

[[burnchain.epochs]]
epoch_name = "2.0"
start_height = {epoch_2_0}

[[burnchain.epochs]]
epoch_name = "2.05"
start_height = {epoch_2_05}

[[burnchain.epochs]]
epoch_name = "2.1"
start_height = {epoch_2_1}
                    "#
        ));
        let config_map: ConfigMap = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": CONFIGMAP_NAME,
                "namespace": namespace
            },
            "data": {
                "Stacks.toml": stacks_conf

            },
        }))?;

        let post_params = PostParams::default();
        let created_config = config_map_api.create(&post_params, &config_map).await?;
        let name = created_config.name_any();
        assert_eq!(config_map.name_any(), name);
        println!("Created {}", name);
    }

    // deploy stacks node pod
    {
        let client = Client::try_default().await?;
        let pods_api: Api<Pod> = Api::namespaced(client, &namespace);

        let bitcoin_pod: Pod = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": POD_NAME,
            "namespace": namespace,
            "labels": {"name": POD_NAME}
        },
        "spec": {
            "containers": Some(vec![ Container {
                name: CONTAINER_NAME.into(),
                image: Some(image.into()),
                command: Some(vec![
                    "stacks-node".into(),
                    "start".into(),
                    "--config=/src/stacks-node/Stacks.toml".into(),
                ]),
                ports: Some(vec![
                    ContainerPort {
                        container_port: p2p_port,
                        protocol: Some("TCP".into()),
                        name: Some("p2p".into()),
                        ..Default::default()
                    },
                    ContainerPort {
                        container_port: rpc_port,
                        protocol: Some("TCP".into()),
                        name: Some("rpc".into()),
                        ..Default::default()
                    },
                ]),
                env: Some(vec![
                    EnvVar {
                        name: String::from("STACKS_LOG_PP"),
                        value: Some(String::from("1")),
                        ..Default::default()
                    },
                    EnvVar {
                        name: String::from("BLOCKSTACK_USE_TEST_GENESIS_CHAINSTATE"),
                        value: Some(String::from("1")),
                        ..Default::default()
                    },
                    EnvVar {
                        name: String::from("STACKS_LOG_DEBUG"),
                        value: Some(String::from("0")),
                        ..Default::default()
                    }
                ]),
                volume_mounts: Some(vec![ VolumeMount {
                    name: CONFIGMAP_VOLUME_NAME.into(),
                    mount_path: "/src/stacks-node".into(),
                    read_only: Some(true),
                    ..Default::default()
                }]),
                ..Default::default()
            }]),
            "volumes": Some(vec![
                Volume {
                name: CONFIGMAP_VOLUME_NAME.into(),
                config_map: Some(ConfigMapVolumeSource {
                    name: Some(CONFIGMAP_NAME.into())
                    , ..Default::default()
                }),
                ..Default::default()
            }])
        }}))?;

        let pp = PostParams::default();
        let response = pods_api.create(&pp, &bitcoin_pod).await?;
        let name = response.name_any();
        println!("created pod {}", name);
    }

    // deploy service to communicate with container
    {
        let client = Client::try_default().await?;
        let service_api: Api<Service> = Api::namespaced(client, &namespace);

        let mut selector = BTreeMap::<String, String>::new();
        selector.insert("name".into(), CONTAINER_NAME.into());

        let service: Service = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": SERVICE_NAME,
                "namespace": namespace
            },
            "spec":  {
                "type": "NodePort",
                "ports": Some(vec![ServicePort {
                    port: p2p_port,
                    protocol: Some("TCP".into()),
                    name: Some("p2p".into()),
                    node_port: Some(30002),
                    ..Default::default()
                },ServicePort {
                    port: rpc_port,
                    protocol: Some("TCP".into()),
                    name: Some("rpc".into()),
                    node_port: Some(30003),
                    ..Default::default()
                }]),
                "selector":  {"name": POD_NAME},
            }
        }))?;

        let pp = PostParams::default();
        let response = service_api.create(&pp, &service).await?;
        let name = response.name_any();
        println!("created service {}", name);
    }
    Ok(())
}

async fn deploy_stacks_api_pod(
    namespace: &str,
    image: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // constants for stacks pod, services, and config
    const POD_NAME: &str = "stacks-api";
    const POSTGRES_POD_NAME: &str = "stacks-api-postgres";
    const CONTAINER_NAME: &str = "stacks-api-container";
    const POSTGRES_CONFIGMAP_NAME: &str = "stacks-api-postgres-conf";
    const PVC_NAME: &str = "stacks-api-pvc";
    const STORAGE_CLASS_NAME: &str = "stacks-api-storage-class";
    const POSTGRES_CONFIGMAP_VOLUME_NAME: &str = "stacks-api-postgres-conf-volume";
    const SERVICE_NAME: &str = "stacks-api-service";

    // deploy config map for stacks api
    {
        let client = Client::try_default().await?;
        let config_map_api: Api<ConfigMap> = kube::Api::<ConfigMap>::namespaced(client, &namespace);

        let config_map: ConfigMap = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": POSTGRES_CONFIGMAP_NAME,
                "namespace": namespace
            },
            "data": {
                "POSTGRES_PASSWORD": "postgres",
                "POSTGRES_DB": "stacks_api",

            },
        }))?;

        let post_params = PostParams::default();
        let created_config = config_map_api.create(&post_params, &config_map).await?;
        let name = created_config.name_any();
        assert_eq!(config_map.name_any(), name);
        println!("Created {}", name);
    }

    // deploy storage class
    // {
    //     let client = Client::try_default().await?;
    //     let storage_class_api: Api<StorageClass> = kube::Api::all(client);

    //     let storage_class: StorageClass = serde_json::from_value(json!({
    //         "apiVersion": "storage.k8s.io/v1",
    //         "kind": "StorageClass",
    //         "metadata": {
    //             "name": STORAGE_CLASS_NAME,
    //             "namespace": namespace,
    //             "labels": {
    //                 "app": "my-app",
    //             },
    //             "annotations": {
    //                 "openebs.io/cas-type": "local",
    //                 "cas.openebs.io/config": "|
    //                 - name: StorageType
    //                   value: hostpath
    //                 - name: BasePath
    //                   value: /var/local-hostpath"
    //             }
    //         },
    //         "provisioner": "openebs.io/local",
    //         "volume_binding_modes": "WaitForFirstConsumer"
    //     }))?;

    //     let post_params = PostParams::default();
    //     let created_config = storage_class_api
    //         .create(&post_params, &storage_class)
    //         .await?;
    //     let name = created_config.name_any();
    //     assert_eq!(storage_class.name_any(), name);
    //     println!("Created {}", name);
    // }

    // deploy persistent volume claim
    {
        let client = Client::try_default().await?;
        let pvc_api: Api<PersistentVolumeClaim> =
            kube::Api::<PersistentVolumeClaim>::namespaced(client, &namespace);

        let mut requests_map: BTreeMap<String, Quantity> = BTreeMap::new();
        requests_map.insert("storage".to_string(), Quantity("500Mi".to_string()));
        let mut limits_map: BTreeMap<String, Quantity> = BTreeMap::new();
        limits_map.insert("storage".to_string(), Quantity("750Mi".to_string()));

        let pvc: PersistentVolumeClaim = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "PersistentVolumeClaim",
            "metadata": {
                "name": PVC_NAME,
                "namespace": namespace,
            },
            "spec": PersistentVolumeClaimSpec {
                storage_class_name: Some(STORAGE_CLASS_NAME.to_string()),
                access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                resources: Some( ResourceRequirements {
                    requests: Some(requests_map),
                    limits: Some(limits_map),
                }),
                ..Default::default()
            },
        }))?;

        let post_params = PostParams::default();
        let created_config = pvc_api.create(&post_params, &pvc).await?;
        let name = created_config.name_any();
        assert_eq!(pvc.name_any(), name);
        println!("Created {}", name);
    }

    // deploy pod with stacks api and postgres containers
    {
        let client = Client::try_default().await?;
        let pods_api: Api<Pod> = Api::namespaced(client, &namespace);

        let env: Vec<EnvVar> = vec![
            EnvVar {
                name: String::from("STACKS_CORE_RPC_HOST"),
                value: Some(format!("host.docker.internal",)),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_BLOCKCHAIN_API_DB"),
                value: Some(String::from("pg")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_CORE_RPC_PORT"),
                value: Some("30003".to_string()),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_BLOCKCHAIN_API_PORT"),
                value: Some("3999".to_string()),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_BLOCKCHAIN_API_HOST"),
                value: Some(String::from("0.0.0.0")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_CORE_EVENT_PORT"),
                value: Some("3700".to_string()),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_CORE_EVENT_HOST"),
                value: Some(String::from("0.0.0.0")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_API_ENABLE_FT_METADATA"),
                value: Some(String::from("1")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("PG_HOST"),
                value: Some(format!("host.docker.internal",)),
                ..Default::default()
            },
            EnvVar {
                name: String::from("PG_PORT"),
                value: Some(String::from("30006")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("PG_USER"),
                value: Some("postgres".to_string()),
                ..Default::default()
            },
            EnvVar {
                name: String::from("PG_PASSWORD"),
                value: Some("postgres".to_string()),
                ..Default::default()
            },
            EnvVar {
                name: String::from("PG_DATABASE"),
                value: Some("stacks_api".to_string()),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_CHAIN_ID"),
                value: Some(String::from("2147483648")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("V2_POX_MIN_AMOUNT_USTX"),
                value: Some(String::from("90000000260")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("NODE_ENV"),
                value: Some(String::from("production")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_API_LOG_LEVEL"),
                value: Some(String::from("debug")),
                ..Default::default()
            },
        ];

        let stacks_api_pod: Pod = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": POD_NAME,
            "namespace": namespace,
            "labels": {
                "name": POD_NAME
            }
        },
        "spec": {
            "containers": Some(vec![
                Container {
                    name: CONTAINER_NAME.into(),
                    image: Some(image.into()),
                    image_pull_policy: Some("Never".into()),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: 3999,
                            protocol: Some(String::from("TCP")),
                            name: Some("api".into()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: 3700,
                            protocol: Some(String::from("TCP")),
                            name: Some("eventport".into()),
                            ..Default::default()
                        },
                    ]),
                    env: Some(env),
                    ..Default::default()
                },
                Container {
                    name: POSTGRES_POD_NAME.into(),
                    image: Some("postgres:14".to_string()),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: 5432,
                            protocol: Some(String::from("TCP")),
                            name: Some("postgres".into()),
                            ..Default::default()
                        },
                    ]),
                    env_from: Some(vec![
                        EnvFromSource {
                            config_map_ref: Some( ConfigMapEnvSource{name: Some(POSTGRES_CONFIGMAP_NAME.to_string()), optional: Some(false)}),
                            ..Default::default()
                        }
                    ]),
                    volume_mounts: Some(vec![ VolumeMount {
                        name: POSTGRES_CONFIGMAP_VOLUME_NAME.into(),
                        mount_path: "/var/lib/postgresql/data".into(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }]),
                "volumes": Some(vec![
                    Volume {
                    name: POSTGRES_CONFIGMAP_VOLUME_NAME.into(),
                    persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                        claim_name: PVC_NAME.into()
                        , ..Default::default()
                    }),
                    ..Default::default()
                }])
        }}))?;

        let pp = PostParams::default();
        let response = pods_api.create(&pp, &stacks_api_pod).await?;
        let name = response.name_any();
        println!("created pod {}", name);
    }

    // deploy service to communicate with container
    {
        let client = Client::try_default().await?;
        let service_api: Api<Service> = Api::namespaced(client, &namespace);

        let mut selector = BTreeMap::<String, String>::new();
        selector.insert("name".into(), CONTAINER_NAME.into());

        let service: Service = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": SERVICE_NAME,
                "namespace": namespace,
            },
            "spec":  {
                "type": "NodePort",
                "ports": Some(vec![ServicePort {
                    port: 3999,
                    protocol: Some("TCP".into()),
                    name: Some("api".into()),
                    node_port: Some(30005),
                    ..Default::default()
                },
                ServicePort {
                    port: 5432,
                    protocol: Some("TCP".into()),
                    name: Some("postgres".into()),
                    node_port: Some(30006),
                    ..Default::default()
                },
                ServicePort {
                    port: 3700,
                    protocol: Some("TCP".into()),
                    name: Some("eventport".into()),
                    node_port: Some(30007),
                    ..Default::default()
                }]),
                "selector":  {
                    "name": POD_NAME
                },
            }
        }))?;

        let pp = PostParams::default();
        let response = service_api.create(&pp, &service).await?;
        let name = response.name_any();
        println!("created service {}", name);
    }
    Ok(())
}

async fn deploy_chain_coordinator(
    namespace: &str,
    image: &str,
    orchestrator_service_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    const POD_NAME: &str = "chain-coordinator";
    const CONTAINER_NAME: &str = "chain-coordinator-container";
    const CONFIGMAP_NAME: &str = "chain-coordinator-conf";
    const CONFIGMAP_VOLUME_NAME: &str = "chain-coordinator-conf-volume";

    // // deploy config map for chain coordinator
    // {
    //     let client = Client::try_default().await?;
    //     let config_map_api: Api<ConfigMap> = kube::Api::<ConfigMap>::namespaced(client, &namespace);

    //     let manifest_path = PathBuf::from("../stx-px/Clarinet.toml");
    //     let manifest = fs::read_to_string(manifest_path)?;

    //     let deployment_plan_path = PathBuf::from("../stx-px/deployments/default.devnet-plan.yaml");
    //     let deployment_plan = fs::read_to_string(deployment_plan_path)?;

    //     let config_map: ConfigMap = serde_json::from_value(json!({
    //         "apiVersion": "v1",
    //         "kind": "ConfigMap",
    //         "metadata": {
    //             "name": CONFIGMAP_NAME,
    //             "namespace": namespace
    //         },
    //         "data": {
    //             "Clarinet.toml": manifest,
    //             "deployment-plan.yaml": deployment_plan,
    //         },
    //     }))?;

    //     let post_params = PostParams::default();
    //     let created_config = config_map_api.create(&post_params, &config_map).await?;
    //     let name = created_config.name_any();
    //     assert_eq!(config_map.name_any(), name);
    //     println!("Created {}", name);
    // }

    // deploy pod
    {
        let client = Client::try_default().await?;
        let pods_api: Api<Pod> = Api::namespaced(client, &namespace);

        let project_path = String::from("/foo2");

        let pod: Pod = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": POD_NAME,
            "namespace": namespace,
            "labels": {"name": POD_NAME}
        },
        "spec": {
            "containers": Some(vec![ Container {
                name: CONTAINER_NAME.into(),
                image: Some(image.into()),
                image_pull_policy: Some("Never".into()),
                command: Some(vec![
                    "./stacks-network".into(),
                    // "--help".into(),
                    "--manifest-path=/etc/stacks-network/project/Clarinet.toml".into(),
                    "--deployment-plan-path=/etc/stacks-network/project/deployments/default.devnet-plan.yaml".into(),
                    "--project-root-path=/etc/stacks-network/project/".into(),
                ]),
                ports: Some(vec![
                    ContainerPort {
                        container_port: 20445,
                        protocol: Some("TCP".into()),
                        name: Some("orchestrator".into()),
                        ..Default::default()
                    },
                    ContainerPort {
                        container_port: 20446,
                        protocol: Some("TCP".into()),
                        name: Some("orch-control".into()),
                        ..Default::default()
                    },
                ]),
                volume_mounts: Some(vec![
                    // VolumeMount {
                    //     name: CONFIGMAP_VOLUME_NAME.into(),
                    //     mount_path: "/etc/stacks-network".into(),
                    //     read_only: Some(true),
                    //     ..Default::default()
                    // },
                    VolumeMount {
                        name: "project".into(),
                        mount_path: "/etc/stacks-network/project".into(),
                        read_only: Some(false),
                        ..Default::default()
                    }
                ]),
                ..Default::default()
            }]),
            "volumes": Some(vec![
                // Volume {
                //     name: CONFIGMAP_VOLUME_NAME.into(),
                //     config_map: Some(ConfigMapVolumeSource {
                //         name: Some(CONFIGMAP_NAME.into())
                //         , ..Default::default()
                //     }),
                //     ..Default::default()
                // },
                Volume {
                    name: "project".into(),
                    host_path: Some(HostPathVolumeSource { path: project_path, type_: Some("Directory".into())}),
                    ..Default::default()
                }
            ])
        }}))?;

        let pp = PostParams::default();
        let response = pods_api.create(&pp, &pod).await?;
        let name = response.name_any();
        println!("created pod {}", name);
    }

    // deploy service to communicate with container
    {
        let client = Client::try_default().await?;
        let service_api: Api<Service> = Api::namespaced(client, &namespace);

        let mut selector = BTreeMap::<String, String>::new();
        selector.insert("name".into(), CONTAINER_NAME.into());

        let service: Service = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": orchestrator_service_name,
                "namespace": namespace
            },
            "spec":  {
                "type": "NodePort",
                "ports": Some(vec![ServicePort {
                    port: 20445,
                    protocol: Some("TCP".into()),
                    name: Some("orchestrator".into()),
                    node_port: Some(30008),
                    ..Default::default()
                },ServicePort {
                    port: 20446,
                    protocol: Some("TCP".into()),
                    name: Some("orch-control".into()),
                    node_port: Some(30009),
                    ..Default::default()
                }]),
                "selector":  {"name": POD_NAME},
            }
        }))?;

        let pp = PostParams::default();
        let response = service_api.create(&pp, &service).await?;
        let name = response.name_any();
        println!("created service {}", name);
    }
    Ok(())
}
