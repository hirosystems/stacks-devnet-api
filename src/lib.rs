use chainhook_types::StacksNetwork;
use clarinet_files::{
    compute_addresses, DEFAULT_DERIVATION_PATH, DEFAULT_EPOCH_2_0, DEFAULT_EPOCH_2_05,
    DEFAULT_EPOCH_2_1, DEFAULT_POX2_ACTIVATION, DEFAULT_STACKS_MINER_MNEMONIC,
};
use futures::future::try_join4;
use hiro_system_kit::{slog, Logger};
use hyper::{body::Bytes, Body, Client as HttpClient, Request, Response, Uri};
use k8s_openapi::{
    api::core::v1::{ConfigMap, Namespace, PersistentVolumeClaim, Pod, Service},
    NamespaceResourceScope,
};
use kube::{
    api::{Api, DeleteParams, PostParams},
    Client,
};
use resources::{
    pvc::StacksDevnetPvc,
    service::{get_service_port, ServicePort},
    StacksDevnetResource,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::thread::sleep;
use std::{collections::BTreeMap, str::FromStr, time::Duration};
use strum::IntoEnumIterator;
use tower::BoxError;

pub mod config;
use config::{StacksDevnetConfig, ValidatedStacksDevnetConfig};

mod template_parser;
use template_parser::get_yaml_from_resource;

pub mod resources;
pub mod routes;
use crate::resources::configmap::StacksDevnetConfigmap;
use crate::resources::pod::StacksDevnetPod;
use crate::resources::service::{get_service_url, StacksDevnetService};

pub struct DevNetError {
    pub message: String,
    pub code: u16,
}

#[derive(Clone)]
pub struct Context {
    pub logger: Option<Logger>,
    pub tracer: bool,
}

impl Context {
    pub fn empty() -> Context {
        Context {
            logger: None,
            tracer: false,
        }
    }

    pub fn try_log<F>(&self, closure: F)
    where
        F: FnOnce(&Logger),
    {
        if let Some(ref logger) = self.logger {
            closure(logger)
        }
    }

    pub fn expect_logger(&self) -> &Logger {
        self.logger.as_ref().unwrap()
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StacksDevnetInfoResponse {
    bitcoind_node_status: Option<String>,
    stacks_node_status: Option<String>,
    stacks_api_status: Option<String>,
    bitcoind_node_started_at: Option<String>,
    stacks_node_started_at: Option<String>,
    stacks_api_started_at: Option<String>,
    stacks_chain_tip: u64,
    bitcoin_chain_tip: u64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct PodStatusResponse {
    status: Option<String>,
    start_time: Option<String>,
}
#[derive(Serialize, Deserialize, Debug, Default)]
struct StacksV2InfoResponse {
    burn_block_height: u64,
    stacks_tip_height: u64,
}
#[derive(Clone)]
pub struct StacksDevnetApiK8sManager {
    client: Client,
    ctx: Context,
}

impl StacksDevnetApiK8sManager {
    pub async fn default(ctx: &Context) -> StacksDevnetApiK8sManager {
        let client = Client::try_default()
            .await
            .expect("could not create kube client");
        StacksDevnetApiK8sManager {
            client,
            ctx: ctx.to_owned(),
        }
    }

    pub async fn new<S, B, T>(
        service: S,
        default_namespace: T,
        ctx: &Context,
    ) -> StacksDevnetApiK8sManager
    where
        S: tower::Service<Request<Body>, Response = Response<B>> + Send + 'static,
        S::Future: Send + 'static,
        S::Error: Into<BoxError>,
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
        T: Into<String>,
    {
        let client = Client::new(service, default_namespace);
        StacksDevnetApiK8sManager {
            client,
            ctx: ctx.to_owned(),
        }
    }

    pub async fn deploy_devnet(
        &self,
        config: ValidatedStacksDevnetConfig,
    ) -> Result<(), DevNetError> {
        let user_config = config.user_config;
        let namespace = &user_config.namespace;

        let context = format!("NAMESPACE: {}", &namespace);
        let namespace_exists = self.check_namespace_exists(&namespace).await?;
        if !namespace_exists {
            if cfg!(debug_assertions) {
                self.deploy_namespace(&namespace).await?;
            } else {
                let msg = format!(
                    "cannot create devnet because namespace {} does not exist",
                    namespace
                );
                self.ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
                return Err(DevNetError {
                    message: msg.into(),
                    code: 400,
                });
            }
        }

        let any_assets_exist = self.check_any_devnet_assets_exist(&namespace).await?;
        if any_assets_exist {
            let msg = format!(
                "cannot create devnet because assets already exist {}",
                context
            );
            self.ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
            return Err(DevNetError {
                message: msg.into(),
                code: 409,
            });
        };

        self.deploy_bitcoin_node_pod(
            &user_config,
            config.project_manifest_yaml_string,
            config.network_manifest_yaml_string,
            config.deployment_plan_yaml_string,
            config.contract_configmap_data,
        )
        .await?;

        sleep(Duration::from_secs(5));

        self.deploy_stacks_node_pod(&user_config).await?;

        if !user_config.disable_stacks_api {
            self.deploy_stacks_api_pod(&namespace).await?;
        }
        Ok(())
    }

    pub async fn delete_devnet(&self, namespace: &str) -> Result<(), Box<dyn std::error::Error>> {
        let pods: Vec<String> = StacksDevnetPod::iter().map(|p| p.to_string()).collect();
        for pod in pods {
            let _ = self.delete_resource::<Pod>(namespace, &pod).await;
        }

        let configmaps: Vec<String> = StacksDevnetConfigmap::iter()
            .map(|c| c.to_string())
            .collect();
        for configmap in configmaps {
            let _ = self
                .delete_resource::<ConfigMap>(namespace, &configmap)
                .await;
        }

        let services: Vec<String> = StacksDevnetService::iter().map(|s| s.to_string()).collect();
        for service in services {
            let _ = self.delete_resource::<Service>(namespace, &service).await;
        }

        let pvcs: Vec<String> = StacksDevnetPvc::iter().map(|s| s.to_string()).collect();
        for pvc in pvcs {
            let _ = self
                .delete_resource::<PersistentVolumeClaim>(namespace, &pvc)
                .await;
        }
        Ok(())
    }

    pub async fn check_namespace_exists(&self, namespace_str: &str) -> Result<bool, DevNetError> {
        self.ctx.try_log(|logger| {
            slog::warn!(
                logger,
                "checking if namespace NAMESPACE: {}",
                &namespace_str
            )
        });
        let namespace_api: Api<Namespace> = kube::Api::all(self.client.to_owned());
        match namespace_api.get(namespace_str).await {
            Ok(_) => Ok(true),
            Err(kube::Error::Api(api_error)) => {
                if api_error.code == 404 {
                    Ok(false)
                } else {
                    let msg = format!(
                        "error getting namespace {}: {}",
                        namespace_str, api_error.message
                    );
                    self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                    Err(DevNetError {
                        message: msg,
                        code: api_error.code,
                    })
                }
            }
            Err(e) => {
                let msg = format!(
                    "error getting namespace {}: {}",
                    namespace_str,
                    e.to_string()
                );
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: 500,
                })
            }
        }
    }

    pub async fn check_any_devnet_assets_exist(
        &self,
        namespace: &str,
    ) -> Result<bool, DevNetError> {
        self.ctx.try_log(|logger| {
            slog::warn!(
                logger,
                "checking if any devnet assets exist for devnet NAMESPACE: {}",
                &namespace
            )
        });
        for pod in StacksDevnetPod::iter() {
            if self
                .check_resource_exists::<Pod>(namespace, &pod.to_string())
                .await?
            {
                return Ok(true);
            }
        }

        for configmap in StacksDevnetConfigmap::iter() {
            if self
                .check_resource_exists::<ConfigMap>(namespace, &configmap.to_string())
                .await?
            {
                return Ok(true);
            }
        }

        for service in StacksDevnetService::iter() {
            if self
                .check_resource_exists::<Service>(namespace, &service.to_string())
                .await?
            {
                return Ok(true);
            }
        }

        for pvc in StacksDevnetPvc::iter() {
            if self
                .check_resource_exists::<PersistentVolumeClaim>(namespace, &pvc.to_string())
                .await?
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn get_pod_status_info(
        &self,
        namespace: &str,
        pod: StacksDevnetPod,
    ) -> Result<PodStatusResponse, DevNetError> {
        let context = format!("NAMESPACE: {}, POD: {}", namespace, pod);

        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
            slog::info!(logger, "getting pod status {}", context)
        });
        let pod_api: Api<Pod> = Api::namespaced(self.client.to_owned(), &namespace);
        let pod_name = pod.to_string();
        match pod_api.get_status(&pod_name).await {
            Ok(pod_with_status) => match pod_with_status.status {
                Some(status) => {
                    self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
                        slog::info!(logger, "successfully retrieved pod status {}", context)
                    });
                    let start_time = match status.start_time {
                        Some(st) => Some(st.0.to_string()),
                        None => None,
                    };
                    Ok(PodStatusResponse {
                        status: status.phase,
                        start_time,
                    })
                }
                None => Ok(PodStatusResponse::default()),
            },
            Err(e) => {
                let result = match e {
                    kube::Error::Api(api_error) => {
                        let code = api_error.code;
                        if code == 404 {
                            Ok(PodStatusResponse {
                                status: Some("Not found".into()),
                                ..Default::default()
                            })
                        } else {
                            Err((api_error.message, code))
                        }
                    }
                    e => Err((e.to_string(), 500)),
                };
                match result {
                    Ok(r) => Ok(r),
                    Err((msg, code)) => {
                        let msg = format!("failed to get pod status {}, ERROR: {}", context, msg);
                        self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                        Err(DevNetError { message: msg, code })
                    }
                }
            }
        }
    }

    async fn get_stacks_v2_info(
        &self,
        namespace: &str,
    ) -> Result<StacksV2InfoResponse, DevNetError> {
        let client = HttpClient::new();
        let url = get_service_url(namespace, StacksDevnetService::StacksNode);
        let port = get_service_port(StacksDevnetService::StacksNode, ServicePort::RPC).unwrap();
        let url = format!("http://{}:{}/v2/info", url, port);

        let context = format!("NAMESPACE: {}", namespace);
        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
            slog::info!(
                logger,
                "requesting /v2/info route of stacks node {}",
                context
            )
        });

        match Uri::from_str(&url) {
            Ok(uri) => match client.get(uri).await {
                Ok(response) => match hyper::body::to_bytes(response.into_body()).await {
                    Ok(body) => match serde_json::from_slice::<StacksV2InfoResponse>(&body) {
                        Ok(config) => {
                            self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
                                slog::info!(
                                    logger,
                                    "successfully requested /v2/info route of stacks node {}",
                                    context
                                )
                            });
                            Ok(config)
                        }
                        Err(e) => {
                            let msg = format!(
                                "failed to parse response: {}, ERROR: {}",
                                context,
                                e.to_string()
                            );
                            self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                            Err(DevNetError {
                                message: msg,
                                code: 500,
                            })
                        }
                    },
                    Err(e) => {
                        let msg = format!(
                            "failed to parse response: {}, ERROR: {}",
                            context,
                            e.to_string()
                        );
                        self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                        Err(DevNetError {
                            message: msg,
                            code: 500,
                        })
                    }
                },
                Err(e) => {
                    let msg = format!(
                        "failed to query stacks node: {}, ERROR: {}",
                        context,
                        e.to_string()
                    );
                    self.ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
                    Ok(StacksV2InfoResponse::default())
                }
            },
            Err(e) => {
                let msg = format!("failed to parse url: {} ERROR: {}", context, e.to_string());
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: 500,
                })
            }
        }
    }

    pub async fn get_devnet_info(
        &self,
        namespace: &str,
    ) -> Result<StacksDevnetInfoResponse, DevNetError> {
        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
            slog::info!(logger, "getting devnet info NAMESPACE: {}", namespace)
        });

        let (
            PodStatusResponse {
                status: bitcoind_node_status,
                start_time: bitcoind_node_started_at,
            },
            PodStatusResponse {
                status: stacks_node_status,
                start_time: stacks_node_started_at,
            },
            PodStatusResponse {
                status: stacks_api_status,
                start_time: stacks_api_started_at,
            },
            chain_info,
        ) = try_join4(
            self.get_pod_status_info(&namespace, StacksDevnetPod::BitcoindNode),
            self.get_pod_status_info(&namespace, StacksDevnetPod::StacksNode),
            self.get_pod_status_info(&namespace, StacksDevnetPod::StacksApi),
            self.get_stacks_v2_info(&namespace),
        )
        .await?;

        Ok(StacksDevnetInfoResponse {
            bitcoind_node_status,
            stacks_node_status,
            stacks_api_status,
            bitcoind_node_started_at,
            stacks_node_started_at,
            stacks_api_started_at,
            stacks_chain_tip: chain_info.stacks_tip_height,
            bitcoin_chain_tip: chain_info.burn_block_height,
        })
    }

    async fn deploy_namespace(&self, namespace_str: &str) -> Result<(), DevNetError> {
        let mut namespace: Namespace =
            self.get_resource_from_file(StacksDevnetResource::Namespace)?;

        namespace.metadata.name = Some(namespace_str.to_owned());
        namespace.metadata.labels =
            Some(BTreeMap::from([("name".into(), namespace_str.to_owned())]));

        let namespace_api: Api<Namespace> = kube::Api::all(self.client.to_owned());

        let pp = PostParams::default();

        self.ctx
            .try_log(|logger| slog::info!(logger, "creating namespace {}", namespace_str));
        match namespace_api.create(&pp, &namespace).await {
            Ok(_) => {
                self.ctx.try_log(|logger| {
                    slog::info!(logger, "successfully created namespace {}", namespace_str)
                });
                Ok(())
            }
            Err(e) => {
                let e = match e {
                    kube::Error::Api(api_error) => (api_error.message, api_error.code),
                    e => (e.to_string(), 500),
                };
                let msg = format!("failed to create namespace {}: {}", namespace_str, e.0);
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: e.1,
                })
            }
        }
    }

    async fn get_resource<K: kube::Resource<Scope = NamespaceResourceScope>>(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<K>, DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: Serialize,
    {
        let resource_api: Api<K> = Api::namespaced(self.client.to_owned(), &namespace);

        let resource_details = format!(
            "RESOURCE: {}, NAME: {}, NAMESPACE: {}",
            std::any::type_name::<K>(),
            name,
            namespace
        );
        self.ctx
            .try_log(|logger| slog::info!(logger, "fetching {}", resource_details));

        match resource_api.get_opt(&name).await {
            Ok(r) => match r {
                Some(r) => {
                    self.ctx.try_log(|logger| {
                        slog::info!(logger, "successfully fetched {}", resource_details)
                    });
                    Ok(Some(r))
                }
                None => {
                    self.ctx.try_log(|logger| {
                        slog::info!(logger, "resource not found {}", resource_details)
                    });
                    Ok(None)
                }
            },
            Err(e) => {
                let (msg, code) = match e {
                    kube::Error::Api(api_error) => (api_error.message, api_error.code),
                    e => (e.to_string(), 500),
                };
                let msg = format!("failed to fetch {}, ERROR: {}", resource_details, msg);
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: code,
                })
            }
        }
    }

    async fn check_resource_exists<K: kube::Resource<Scope = NamespaceResourceScope>>(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<bool, DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: Serialize,
    {
        match self.get_resource::<K>(namespace, name).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    async fn deploy_resource<K: kube::Resource<Scope = NamespaceResourceScope>>(
        &self,
        namespace: &str,
        resource: K,
        resource_type: &str,
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

        let name = match resource.meta().name.as_ref() {
            Some(name) => name,
            None => {
                self.ctx.try_log(|logger| {
                    slog::warn!(
                        logger,
                        "resource does not have a name field. it really should"
                    )
                });
                "no-name"
            }
        };
        let resource_details = format!(
            "RESOURCE: {}, NAME: {}, NAMESPACE: {}",
            resource_type, name, namespace
        );
        self.ctx
            .try_log(|logger| slog::info!(logger, "creating {}", resource_details));

        match resource_api.create(&pp, &resource).await {
            Ok(_) => {
                self.ctx.try_log(|logger| {
                    slog::info!(logger, "successfully created {}", resource_details)
                });
                Ok(())
            }
            Err(e) => {
                let e = match e {
                    kube::Error::Api(api_error) => (api_error.message, api_error.code),
                    e => (e.to_string(), 500),
                };
                let msg = format!("failed to create {}, ERROR: {}", resource_details, e.0);
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: e.1,
                })
            }
        }
    }

    async fn deploy_pod(&self, pod: StacksDevnetPod, namespace: &str) -> Result<(), DevNetError> {
        let mut pod: Pod = self.get_resource_from_file(StacksDevnetResource::Pod(pod))?;

        pod.metadata.namespace = Some(namespace.to_owned());
        self.deploy_resource(namespace, pod, "pod").await
    }

    async fn deploy_service(
        &self,
        service: StacksDevnetService,
        namespace: &str,
    ) -> Result<(), DevNetError> {
        let mut service: Service =
            self.get_resource_from_file(StacksDevnetResource::Service(service))?;

        service.metadata.namespace = Some(namespace.to_owned());
        self.deploy_resource(namespace, service, "service").await
    }

    async fn deploy_configmap(
        &self,
        configmap: StacksDevnetConfigmap,
        namespace: &str,
        configmap_data: Option<Vec<(String, String)>>,
    ) -> Result<(), DevNetError> {
        let mut configmap: ConfigMap =
            self.get_resource_from_file(StacksDevnetResource::Configmap(configmap))?;

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

    async fn deploy_pvc(&self, pvc: StacksDevnetPvc, namespace: &str) -> Result<(), DevNetError> {
        let mut pvc: PersistentVolumeClaim =
            self.get_resource_from_file(StacksDevnetResource::Pvc(pvc))?;

        pvc.metadata.namespace = Some(namespace.to_owned());

        self.deploy_resource(namespace, pvc, "pvc").await
    }

    async fn deploy_bitcoin_node_pod(
        &self,
        config: &StacksDevnetConfig,
        project_mainfest: String,
        network_manifest: String,
        deployment_plan: String,
        contracts: Vec<(String, String)>,
    ) -> Result<(), DevNetError> {
        let namespace = &config.namespace;

        let bitcoin_rpc_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::RPC).unwrap();
        let bitcoin_p2p_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::P2P).unwrap();

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
            bitcoin_p2p_port,
            bitcoin_rpc_port,
            bitcoin_rpc_port
        );

        self.deploy_configmap(
            StacksDevnetConfigmap::BitcoindNode,
            &namespace,
            Some(vec![("bitcoin.conf".into(), bitcoind_conf)]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::Namespace,
            &namespace,
            Some(vec![("NAMESPACE".into(), namespace.clone())]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::ProjectManifest,
            &namespace,
            Some(vec![("Clarinet.toml".into(), project_mainfest)]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::Devnet,
            &namespace,
            Some(vec![("Devnet.toml".into(), network_manifest)]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::DeploymentPlan,
            &namespace,
            Some(vec![("default.devnet-plan.yaml".into(), deployment_plan)]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::ProjectDir,
            &namespace,
            Some(contracts),
        )
        .await?;

        self.deploy_pod(StacksDevnetPod::BitcoindNode, &namespace)
            .await?;

        self.deploy_service(StacksDevnetService::BitcoindNode, namespace)
            .await?;

        Ok(())
    }

    async fn deploy_stacks_node_pod(&self, config: &StacksDevnetConfig) -> Result<(), DevNetError> {
        let namespace = &config.namespace;

        let chain_coordinator_ingestion_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Ingestion).unwrap();

        let (miner_coinbase_recipient, _, stacks_miner_secret_key_hex) = compute_addresses(
            &config
                .miner_mnemonic
                .clone()
                .unwrap_or(DEFAULT_STACKS_MINER_MNEMONIC.into()),
            &config
                .miner_derivation_path
                .clone()
                .unwrap_or(DEFAULT_DERIVATION_PATH.into()),
            &StacksNetwork::Devnet.get_networks(),
        );

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
                get_service_port(StacksDevnetService::StacksNode, ServicePort::RPC).unwrap(),
                get_service_port(StacksDevnetService::StacksNode, ServicePort::P2P).unwrap(),
                stacks_miner_secret_key_hex,
                stacks_miner_secret_key_hex,
                config.stacks_node_wait_time_for_microblocks.unwrap_or(50),
                config.stacks_node_first_attempt_time_ms.unwrap_or(500),
                config
                    .stacks_node_subsequent_attempt_time_ms
                    .unwrap_or(1_000),
                miner_coinbase_recipient
            );

            for account in config.accounts.clone().iter() {
                let derivation_path = account
                    .derivation
                    .clone()
                    .unwrap_or(DEFAULT_DERIVATION_PATH.into());
                let (stx_address, _, _) = compute_addresses(
                    &account.mnemonic,
                    &derivation_path,
                    &StacksNetwork::Devnet.get_networks(),
                );
                stacks_conf.push_str(&format!(
                    r#"
                    [[ustx_balance]]
                    address = "{}"
                    amount = {}
                "#,
                    stx_address, account.balance
                ));
            }

            let balance: u64 = 100_000_000_000_000;
            stacks_conf.push_str(&format!(
                r#"
                [[ustx_balance]]
                address = "{}"
                amount = {}
                "#,
                miner_coinbase_recipient, balance
            ));

            let bitcoind_chain_coordinator_host =
                get_service_url(&namespace, StacksDevnetService::BitcoindNode);

            stacks_conf.push_str(&format!(
                r#"
                # Add orchestrator (docker-host) as an event observer
                [[events_observer]]
                endpoint = "{}:{}"
                retry_count = 255
                include_data_events = true
                events_keys = ["*"]
                "#,
                bitcoind_chain_coordinator_host, chain_coordinator_ingestion_port
            ));

            stacks_conf.push_str(&format!(
                r#"
            # Add stacks-api as an event observer
            [[events_observer]]
            endpoint = "{}:{}"
            retry_count = 255
            include_data_events = false
            events_keys = ["*"]
            "#,
                get_service_url(&namespace, StacksDevnetService::StacksApi),
                get_service_port(StacksDevnetService::StacksApi, ServicePort::Event).unwrap(),
            ));

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
                chain_coordinator_ingestion_port,
                get_service_port(StacksDevnetService::BitcoindNode, ServicePort::P2P).unwrap()
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

                # [[burnchain.epochs]]
                # epoch_name = "2.2"
                # start_height = {}
                "#,
                config.pox_2_activation.unwrap_or(DEFAULT_POX2_ACTIVATION),
                config.epoch_2_0.unwrap_or(DEFAULT_EPOCH_2_0),
                config.epoch_2_05.unwrap_or(DEFAULT_EPOCH_2_05),
                config.epoch_2_1.unwrap_or(DEFAULT_EPOCH_2_1),
                config.epoch_2_2.unwrap_or(110) //todo - get default value and uncomment config once stacks image is updated
            ));
            stacks_conf
        };

        self.deploy_configmap(
            StacksDevnetConfigmap::StacksNode,
            &namespace,
            Some(vec![("Stacks.toml".into(), stacks_conf)]),
        )
        .await?;

        self.deploy_pod(StacksDevnetPod::StacksNode, &namespace)
            .await?;

        self.deploy_service(StacksDevnetService::StacksNode, namespace)
            .await?;

        Ok(())
    }

    async fn deploy_stacks_api_pod(&self, namespace: &str) -> Result<(), DevNetError> {
        // configmap env vars for pg conatainer
        let stacks_api_pg_env = Vec::from([
            ("POSTGRES_PASSWORD".into(), "postgres".into()),
            ("POSTGRES_DB".into(), "stacks_api".into()),
        ]);
        self.deploy_configmap(
            StacksDevnetConfigmap::StacksApiPostgres,
            &namespace,
            Some(stacks_api_pg_env),
        )
        .await?;

        // configmap env vars for api conatainer
        let stacks_node_host = get_service_url(&namespace, StacksDevnetService::StacksNode);
        let rpc_port = get_service_port(StacksDevnetService::StacksNode, ServicePort::RPC).unwrap();
        let api_port = get_service_port(StacksDevnetService::StacksApi, ServicePort::API).unwrap();
        let event_port =
            get_service_port(StacksDevnetService::StacksApi, ServicePort::Event).unwrap();
        let db_port = get_service_port(StacksDevnetService::StacksApi, ServicePort::DB).unwrap();
        let stacks_api_env = Vec::from([
            ("STACKS_CORE_RPC_HOST".into(), stacks_node_host),
            ("STACKS_BLOCKCHAIN_API_DB".into(), "pg".into()),
            ("STACKS_CORE_RPC_PORT".into(), rpc_port),
            ("STACKS_BLOCKCHAIN_API_PORT".into(), api_port),
            ("STACKS_BLOCKCHAIN_API_HOST".into(), "0.0.0.0".into()),
            ("STACKS_CORE_EVENT_PORT".into(), event_port),
            ("STACKS_CORE_EVENT_HOST".into(), "0.0.0.0".into()),
            ("STACKS_API_ENABLE_FT_METADATA".into(), "1".into()),
            ("PG_HOST".into(), "0.0.0.0".into()),
            ("PG_PORT".into(), db_port),
            ("PG_USER".into(), "postgres".into()),
            ("PG_PASSWORD".into(), "postgres".into()),
            ("PG_DATABASE".into(), "stacks_api".into()),
            ("STACKS_CHAIN_ID".into(), "2147483648".into()),
            ("V2_POX_MIN_AMOUNT_USTX".into(), "90000000260".into()),
            ("NODE_ENV".into(), "production".into()),
            ("STACKS_API_LOG_LEVEL".into(), "debug".into()),
        ]);
        self.deploy_configmap(
            StacksDevnetConfigmap::StacksApi,
            &namespace,
            Some(stacks_api_env),
        )
        .await?;

        self.deploy_pvc(StacksDevnetPvc::StacksApi, &namespace)
            .await?;

        self.deploy_pod(StacksDevnetPod::StacksApi, &namespace)
            .await?;

        self.deploy_service(StacksDevnetService::StacksApi, &namespace)
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

        let resource_details = format!(
            "RESOURCE: {}, NAME: {}, NAMESPACE: {}",
            std::any::type_name::<K>(),
            resource_name,
            namespace
        );
        self.ctx
            .try_log(|logger| slog::info!(logger, "deleting {}", resource_details));
        match api.delete(resource_name, &dp).await {
            Ok(_) => {
                self.ctx.try_log(|logger| {
                    slog::info!(logger, "successfully deleted {}", resource_details)
                });
                Ok(())
            }
            Err(e) => {
                let e = match e {
                    kube::Error::Api(api_error) => (api_error.message, api_error.code),
                    e => (e.to_string(), 500),
                };
                let msg = format!("failed to delete {}, ERROR: {}", resource_details, e.0);
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: e.1,
                })
            }
        }
    }

    fn get_resource_from_file<K>(&self, template: StacksDevnetResource) -> Result<K, DevNetError>
    where
        K: DeserializeOwned,
    {
        let template_str = get_yaml_from_resource(template);

        match serde_yaml::from_str(template_str) {
            Ok(resource) => Ok(resource),
            Err(e) => {
                let msg = format!("unable to parse template file: {}", e.to_string());
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: 500,
                })
            }
        }
    }
}
