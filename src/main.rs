// use std::{path::PathBuf, str::FromStr};

use std::{collections::BTreeMap, time::Duration};

// use k8s_experimentation::Thing;
use k8s_openapi::{
    api::core::v1::{
        ConfigMap, ConfigMapEnvSource, ConfigMapVolumeSource, Container, ContainerPort,
        EnvFromSource, EnvVar, HostPathVolumeSource, Namespace, PersistentVolumeClaim,
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

const BITCOIND_CHAIN_COORDINATOR_SERVICE_NAME: &str = "bitcoind-service";
const STACKS_NODE_SERVICE_NAME: &str = "stacks-node-service";
// const CHAIN_COORDINATOR_SERVICE_NAME: &str = "orchestrator-service";

const BITCOIND_P2P_PORT: &str = "18444";
const BITCOIND_RPC_PORT: &str = "18443";
const STACKS_NODE_P2P_PORT: &str = "20444";
const STACKS_NODE_RPC_PORT: &str = "20443";
const CHAIN_COORDINATOR_INGESTION_PORT: &str = "20445";
const CHAIN_COORDINATOR_CONTROL_PORT: &str = "20446";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // values from user config
    // let user_id = "user-id";
    // let user_project_name = "my-project";
    let namespace = "px-devnet"; //format!("{user_id}-{user_project_name}-devnet");
    let stacks_miner_secret_key_hex =
        "7287ba251d44a4d3fd9276c88ce34c5c52a038955511cccaf77e61068649c17801";
    let miner_stx_address = "ST1SJ3DTE5DN7X54YDH5D64R3BCB6A2AG2ZQ8YPD5";
    let stacks_node_wait_time_for_microblocks: u32 = 50;
    let stacks_node_first_attempt_time_ms: u32 = 500;
    let stacks_node_subsequent_attempt_time_ms: u32 = 1_000;
    let pox_2_activation = 112;
    let epoch_2_0 = 100;
    let epoch_2_05 = 102;
    let epoch_2_1 = 106;

    create_namespace(&namespace).await?;
    deploy_bitcoin_node_pod(&namespace).await?;

    // deploy_chain_coordinator(&namespace).await?;

    sleep(Duration::from_secs(10));

    deploy_stacks_node_pod(
        &namespace,
        &stacks_miner_secret_key_hex,
        &stacks_node_wait_time_for_microblocks,
        &stacks_node_first_attempt_time_ms,
        &stacks_node_subsequent_attempt_time_ms,
        &miner_stx_address,
        pox_2_activation,
        epoch_2_0,
        epoch_2_05,
        epoch_2_1,
    )
    .await?;

    deploy_stacks_api_pod(&namespace).await?;

    let client = Client::try_default().await?;
    let pods_api: Api<Pod> = Api::namespaced(client, &namespace);
    // Watch it phase for a few seconds
    let establish = await_condition(pods_api.clone(), "bitcoind", is_pod_running());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(15), establish).await?;

    // Verify we can get it
    println!("Get Pod bitcoind");
    let p1cpy = pods_api.get("bitcoind").await?;
    if let Some(spec) = &p1cpy.spec {
        println!("Got bitcoind pod with containers: {:?}", spec.containers);
    }

    let lp = ListParams::default().fields(&format!("metadata.name={}", "bitcoind")); // only want results for our pod
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

async fn create_namespace(namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::try_default().await?;
    let namespace_api: Api<Namespace> = kube::Api::all(client);

    let namespace: Namespace = serde_json::from_value(json!({
        "apiVersion": "v1",
        "kind": "Namespace",
        "metadata": {
            "name": namespace,
            "labels": {
                "name": namespace
            }
        }
    }))?;
    let post_params = PostParams::default();
    let created_namespace = namespace_api.create(&post_params, &namespace).await?;
    let name = created_namespace.name_any();
    assert_eq!(namespace.name_any(), name);
    println!("Created {}", name);
    Ok(())
}

async fn deploy_bitcoin_node_pod(namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    const POD_NAME: &str = "bitcoind-chain-coordinator";
    const BITCOIND_CONTAINER_NAME: &str = "bitcoind-container";
    const BITCOIND_CONFIGMAP_NAME: &str = "bitcoind-conf";
    const BITCOIND_CONFIGMAP_VOLUME_NAME: &str = "bitcoind-conf-volume";
    const BITCOIND_IMAGE: &str = "quay.io/hirosystems/bitcoind:devnet-v3";

    const CHAIN_COORDINATOR_CONTAINER_NAME: &str = "chain-coordinator-container";
    const CHAIN_COORDINATOR_CONFIGMAP_NAME: &str = "chain-coordinator-conf";
    const CHAIN_COORDINATOR_CONFIGMAP_VOLUME_NAME: &str = "chain-coordinator-conf-volume";
    const CHAIN_COORDINATOR_IMAGE: &str = "stacks-network";

    let bitcoind_p2p_port = BITCOIND_P2P_PORT.parse::<i32>()?;
    let bitcoind_rpc_port = BITCOIND_RPC_PORT.parse::<i32>()?;
    let coordinator_ingestion_port = CHAIN_COORDINATOR_INGESTION_PORT.parse::<i32>()?;
    let coordinator_control_port = CHAIN_COORDINATOR_CONTROL_PORT.parse::<i32>()?;

    let project_path = String::from("/foo2");

    // deploy configmap for bitcoin node
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
                bind=0.0.0.0:{bitcoind_p2p_port}
                rpcbind=0.0.0.0:{bitcoind_rpc_port}
                rpcport={bitcoind_rpc_port}
                "#
        );
        let config_map: ConfigMap = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": BITCOIND_CONFIGMAP_NAME,
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

    // deploy  pod
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
            "containers": Some(vec![
                Container {
                    name: BITCOIND_CONTAINER_NAME.into(),
                    image: Some(BITCOIND_IMAGE.into()),
                    command: Some(vec![
                        "/usr/local/bin/bitcoind".into(),
                        "-conf=/etc/bitcoin/bitcoin.conf".into(),
                        "-nodebuglogfile".into(),
                        "-pid=/run/bitcoind.pid".into(),
                    ]),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: bitcoind_p2p_port,
                            protocol: Some("TCP".into()),
                            name: Some("p2p".into()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: bitcoind_rpc_port,
                            protocol: Some("TCP".into()),
                            name: Some("rpc".into()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: coordinator_ingestion_port,
                            protocol: Some("TCP".into()),
                            name: Some("orchestrator".into()),
                            ..Default::default()
                        },
                    ]),
                    volume_mounts: Some(vec![ VolumeMount {
                        name: BITCOIND_CONFIGMAP_VOLUME_NAME.into(),
                        mount_path: "/etc/bitcoin".into(),
                        read_only: Some(true),
                        ..Default::default()
                    }]),
                    ..Default::default()
                },
                Container {
                    name: CHAIN_COORDINATOR_CONTAINER_NAME.into(),
                    image: Some(CHAIN_COORDINATOR_IMAGE.into()),
                    image_pull_policy: Some("Never".into()),
                    command: Some(vec![
                        "./stacks-network".into(),
                        "--manifest-path=/etc/stacks-network/project/Clarinet.toml".into(),
                        "--deployment-plan-path=/etc/stacks-network/project/deployments/default.devnet-plan.yaml".into(),
                        "--project-root-path=/etc/stacks-network/project/".into(),
                    ]),
                    ports: Some(vec![
                        ContainerPort {
                            container_port: coordinator_ingestion_port,
                            protocol: Some("TCP".into()),
                            name: Some("coordinator-in".into()),
                            ..Default::default()
                        },
                        ContainerPort {
                            container_port: coordinator_control_port,
                            protocol: Some("TCP".into()),
                            name: Some("coordinator-con".into()),
                            ..Default::default()
                        },
                    ]),
                    volume_mounts: Some(vec![
                        VolumeMount {
                            name: "project".into(),
                            mount_path: "/etc/stacks-network/project".into(),
                            read_only: Some(false),
                            ..Default::default()
                        }
                    ]),
                    ..Default::default()
                }
            ]),
            "volumes": Some(vec![
                Volume {
                    name: BITCOIND_CONFIGMAP_VOLUME_NAME.into(),
                    config_map: Some(ConfigMapVolumeSource {
                        name: Some(BITCOIND_CONFIGMAP_NAME.into())
                        , ..Default::default()
                    }),
                    ..Default::default()
                },
                Volume {
                    name: "project".into(),
                    host_path: Some(HostPathVolumeSource { path: project_path, type_: Some("Directory".into())}),
                    ..Default::default()
                }
            ])
        }}))?;

        let pp = PostParams::default();
        let response = pods_api.create(&pp, &bitcoin_pod).await?;
        let name = response.name_any();
        println!("created pod {}", name);
    }

    // deploy service
    {
        let client = Client::try_default().await?;
        let service_api: Api<Service> = Api::namespaced(client, &namespace);

        let mut selector = BTreeMap::<String, String>::new();
        selector.insert("name".into(), BITCOIND_CONTAINER_NAME.into());

        let service: Service = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": BITCOIND_CHAIN_COORDINATOR_SERVICE_NAME,
                "namespace": namespace
            },
            "spec":  {
                "ports": Some(vec![
                    ServicePort {
                        port: bitcoind_p2p_port,
                        protocol: Some("TCP".into()),
                        name: Some("p2p".into()),
                        ..Default::default()
                    },
                    ServicePort {
                        port: bitcoind_rpc_port,
                        protocol: Some("TCP".into()),
                        name: Some("rpc".into()),
                        ..Default::default()
                    },
                    ServicePort {
                        port: coordinator_ingestion_port,
                        protocol: Some("TCP".into()),
                        name: Some("coordinator-in".into()),
                        ..Default::default()
                    },
                    ServicePort {
                        port: coordinator_control_port,
                        protocol: Some("TCP".into()),
                        name: Some("coordinator-con".into()),
                        ..Default::default()
                    }
                ]),
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

// async fn deploy_chain_coordinator(namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
//     const POD_NAME: &str = "chain-coordinator";
//     const CONTAINER_NAME: &str = "chain-coordinator-container";

//     let ingestion_port = CHAIN_COORDINATOR_INGESTION_PORT.parse::<i32>()?;
//     let control_port = CHAIN_COORDINATOR_CONTROL_PORT.parse::<i32>()?;

//     // deploy pod
//     {
//         let client = Client::try_default().await?;
//         let pods_api: Api<Pod> = Api::namespaced(client, &namespace);

//         let project_path = String::from("/foo2");

//         let pod: Pod = serde_json::from_value(json!({
//         "apiVersion": "v1",
//         "kind": "Pod",
//         "metadata": {
//             "name": POD_NAME,
//             "namespace": namespace,
//             "labels": {"name": POD_NAME}
//         },
//         "spec": {
//             "containers": Some(vec![ Container {
//                 name: CONTAINER_NAME.into(),
//                 image: Some(CHAIN_COORDINATOR_IMAGE.into()),
//                 image_pull_policy: Some("Never".into()),
//                 command: Some(vec![
//                     "./stacks-network".into(),
//                     "--manifest-path=/etc/stacks-network/project/Clarinet.toml".into(),
//                     "--deployment-plan-path=/etc/stacks-network/project/deployments/default.devnet-plan.yaml".into(),
//                     "--project-root-path=/etc/stacks-network/project/".into(),
//                 ]),
//                 ports: Some(vec![
//                     ContainerPort {
//                         container_port: ingestion_port,
//                         protocol: Some("TCP".into()),
//                         name: Some("coordinator-in".into()),
//                         ..Default::default()
//                     },
//                     ContainerPort {
//                         container_port: control_port,
//                         protocol: Some("TCP".into()),
//                         name: Some("coordinator-con".into()),
//                         ..Default::default()
//                     },
//                 ]),
//                 volume_mounts: Some(vec![
//                     VolumeMount {
//                         name: "project".into(),
//                         mount_path: "/etc/stacks-network/project".into(),
//                         read_only: Some(false),
//                         ..Default::default()
//                     }
//                 ]),
//                 ..Default::default()
//             }]),
//             "volumes": Some(vec![
//                 Volume {
//                     name: "project".into(),
//                     host_path: Some(HostPathVolumeSource { path: project_path, type_: Some("Directory".into())}),
//                     ..Default::default()
//                 }
//             ])
//         }}))?;

//         let pp = PostParams::default();
//         let response = pods_api.create(&pp, &pod).await?;
//         let name = response.name_any();
//         println!("created pod {}", name);
//     }

//     // deploy service
//     {
//         let client = Client::try_default().await?;
//         let service_api: Api<Service> = Api::namespaced(client, &namespace);

//         let mut selector = BTreeMap::<String, String>::new();
//         selector.insert("name".into(), CONTAINER_NAME.into());

//         let service: Service = serde_json::from_value(json!({
//             "apiVersion": "v1",
//             "kind": "Service",
//             "metadata": {
//                 "name": CHAIN_COORDINATOR_SERVICE_NAME,
//                 "namespace": namespace
//             },
//             "spec":  {
//                 "ports": Some(vec![ServicePort {
//                     port: 20445,
//                     protocol: Some("TCP".into()),
//                     name: Some("coordinator-in".into()),
//                     ..Default::default()
//                 },ServicePort {
//                     port: 20446,
//                     protocol: Some("TCP".into()),
//                     name: Some("coordinator-con".into()),
//                     ..Default::default()
//                 }]),
//                 "selector":  {"name": POD_NAME},
//             }
//         }))?;

//         let pp = PostParams::default();
//         let response = service_api.create(&pp, &service).await?;
//         let name = response.name_any();
//         println!("created service {}", name);
//     }
//     Ok(())
// }

async fn deploy_stacks_node_pod(
    namespace: &str,
    miner_secret_key_hex: &str,
    wait_time_for_microblocks: &u32,
    first_attempt_time_ms: &u32,
    subsequent_attempt_time_ms: &u32,
    miner_coinbase_recipient: &str,
    pox_2_activation: i32,
    epoch_2_0: i32,
    epoch_2_05: i32,
    epoch_2_1: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    const POD_NAME: &str = "stacks-node";
    const CONTAINER_NAME: &str = "stacks-node-container";
    const CONFIGMAP_NAME: &str = "stacks-node-conf";
    const CONFIGMAP_VOLUME_NAME: &str = "stacks-node-conf-volume";
    const STACKS_NODE_IMAGE: &str = "quay.io/hirosystems/stacks-node:devnet-v3";

    let p2p_port = STACKS_NODE_P2P_PORT.parse::<i32>()?;
    let rpc_port = STACKS_NODE_RPC_PORT.parse::<i32>()?;

    // deploy configmap
    {
        let client = Client::try_default().await?;
        let config_map_api: Api<ConfigMap> = kube::Api::<ConfigMap>::namespaced(client, &namespace);
        let stacks_conf = {
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
                namespace,
                namespace,
                CHAIN_COORDINATOR_INGESTION_PORT, /* TODO: this is supposed to be the coordinator ingestion port (consider making coorinator/bitcoind same pod) */
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
                pox_2_activation, epoch_2_0, epoch_2_05, epoch_2_1
            ));
            stacks_conf
        };

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

    // deploy pod
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
                image: Some(STACKS_NODE_IMAGE.into()),
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

    // deploy service
    {
        let client = Client::try_default().await?;
        let service_api: Api<Service> = Api::namespaced(client, &namespace);

        let mut selector = BTreeMap::<String, String>::new();
        selector.insert("name".into(), CONTAINER_NAME.into());

        let service: Service = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": STACKS_NODE_SERVICE_NAME,
                "namespace": namespace
            },
            "spec":  {
                "ports": Some(vec![ServicePort {
                    port: p2p_port,
                    protocol: Some("TCP".into()),
                    name: Some("p2p".into()),
                    ..Default::default()
                },ServicePort {
                    port: rpc_port,
                    protocol: Some("TCP".into()),
                    name: Some("rpc".into()),
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

async fn deploy_stacks_api_pod(namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
    // constants for stacks pod, services, and config
    const POD_NAME: &str = "stacks-api";
    const POSTGRES_CONTAINER_NAME: &str = "stacks-api-postgres";
    const API_CONTAINER_NAME: &str = "stacks-api-container";
    const CONFIGMAP_NAME: &str = "stacks-api-conf";
    const CONFIGMAP_VOLUME_NAME: &str = "stacks-api-conf-volume";
    const PVC_NAME: &str = "stacks-api-pvc";
    const STORAGE_CLASS_NAME: &str = "stacks-api-storage-class";
    const SERVICE_NAME: &str = "stacks-api-service";
    const STACKS_API_IMAGE: &str = "hirosystems/stacks-blockchain-api";
    const POSTGRES_IMAGE: &str = "postgres:14";

    // deploy configmap
    {
        let client = Client::try_default().await?;
        let config_map_api: Api<ConfigMap> = kube::Api::<ConfigMap>::namespaced(client, &namespace);

        let config_map: ConfigMap = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": CONFIGMAP_NAME,
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

    // deploy pod
    {
        let client = Client::try_default().await?;
        let pods_api: Api<Pod> = Api::namespaced(client, &namespace);

        let namespaced_host = format!("{}.svc.cluster.local", &namespace);
        let stacks_node_host = format!("{}.{}", &STACKS_NODE_SERVICE_NAME, namespaced_host);

        let env: Vec<EnvVar> = vec![
            EnvVar {
                name: String::from("STACKS_CORE_RPC_HOST"),
                value: Some(format!("{}", stacks_node_host)),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_BLOCKCHAIN_API_DB"),
                value: Some(String::from("pg")),
                ..Default::default()
            },
            EnvVar {
                name: String::from("STACKS_CORE_RPC_PORT"),
                value: Some(STACKS_NODE_RPC_PORT.to_string()),
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
                value: Some(format!("0.0.0.0",)),
                ..Default::default()
            },
            EnvVar {
                name: String::from("PG_PORT"),
                value: Some(String::from("5432")),
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
                    name: API_CONTAINER_NAME.into(),
                    image: Some(STACKS_API_IMAGE.into()),
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
                    name: POSTGRES_CONTAINER_NAME.into(),
                    image: Some(POSTGRES_IMAGE.to_string()),
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
                            config_map_ref: Some( ConfigMapEnvSource{name: Some(CONFIGMAP_NAME.to_string()), optional: Some(false)}),
                            ..Default::default()
                        }
                    ]),
                    volume_mounts: Some(vec![ VolumeMount {
                        name: CONFIGMAP_VOLUME_NAME.into(),
                        mount_path: "/var/lib/postgresql/data".into(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }]),
                "volumes": Some(vec![
                    Volume {
                    name: CONFIGMAP_VOLUME_NAME.into(),
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

    // deploy service
    {
        let client = Client::try_default().await?;
        let service_api: Api<Service> = Api::namespaced(client, &namespace);

        let mut selector = BTreeMap::<String, String>::new();
        selector.insert("name".into(), API_CONTAINER_NAME.into());

        let service: Service = serde_json::from_value(json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": SERVICE_NAME,
                "namespace": namespace,
            },
            "spec":  {
                "ports": Some(vec![ServicePort {
                    port: 3999,
                    protocol: Some("TCP".into()),
                    name: Some("api".into()),
                    ..Default::default()
                },
                ServicePort {
                    port: 5432,
                    protocol: Some("TCP".into()),
                    name: Some("postgres".into()),
                    ..Default::default()
                },
                ServicePort {
                    port: 3700,
                    protocol: Some("TCP".into()),
                    name: Some("eventport".into()),
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
