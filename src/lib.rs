use std::{collections::BTreeMap, str::FromStr, time::Duration};
use std::{env, thread::sleep};

use clarinet_deployments::types::BurnchainEpochConfig;
use clarinet_files::{compute_addresses, StacksNetwork};
use futures::future::try_join3;
use hiro_system_kit::{slog, Logger};
use hyper::{
    body::{Bytes, HttpBody},
    Body, Client as HttpClient, Request, Response, Uri,
};
use k8s_openapi::{
    api::{
        apps::v1::{Deployment, StatefulSet},
        core::v1::{ConfigMap, Namespace, PersistentVolumeClaim, Pod, Service},
    },
    NamespaceResourceScope,
};
use kube::{
    api::{Api, DeleteParams, ListParams, PostParams},
    config::KubeConfigOptions,
    Client, Config,
};
use resources::{
    deployment::StacksDevnetDeployment,
    pvc::StacksDevnetPvc,
    service::{get_service_port, ServicePort},
    stateful_set::{SignerIdx, StacksDevnetStatefulSet},
    StacksDevnetResource,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use strum::IntoEnumIterator;
use tower::BoxError;

pub mod config;
use config::ValidatedStacksDevnetConfig;

mod template_parser;
use template_parser::get_yaml_from_resource;

pub mod api_config;
pub mod resources;
pub mod responder;
pub mod routes;
use crate::resources::configmap::StacksDevnetConfigmap;
use crate::resources::pod::StacksDevnetPod;
use crate::resources::service::{get_service_url, StacksDevnetService};

const COMPONENT_SELECTOR: &str = "app.kubernetes.io/component";
const USER_SELECTOR: &str = "app.kubernetes.io/instance";
const NAME_SELECTOR: &str = "app.kubernetes.io/name";
#[derive(Clone, Debug)]
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
    pub bitcoind_node_status: Option<String>,
    pub stacks_node_status: Option<String>,
    pub stacks_api_status: Option<String>,
    pub bitcoind_node_started_at: Option<String>,
    pub stacks_node_started_at: Option<String>,
    pub stacks_api_started_at: Option<String>,
    pub stacks_chain_tip: u64,
    pub bitcoin_chain_tip: u64,
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
    pub async fn new(ctx: &Context) -> StacksDevnetApiK8sManager {
        let context = match env::var("KUBE_CONTEXT") {
            Ok(context) => Some(context),
            Err(_) => {
                if cfg!(test) {
                    let is_ci = match env::var("GITHUB_ACTIONS") {
                        Ok(is_ci) => is_ci == *"true",
                        Err(_) => false,
                    };
                    if is_ci {
                        None
                    } else {
                        // ensures that if no context is supplied and we're running
                        // tests locally, we deploy to the local kind cluster
                        Some("kind-kind".to_string())
                    }
                } else {
                    None
                }
            }
        };
        let client = match context {
            Some(context) => {
                let kube_config = KubeConfigOptions {
                    context: Some(context.clone()),
                    cluster: Some(context),
                    user: None,
                };
                let client_config = Config::from_kubeconfig(&kube_config)
                    .await
                    .unwrap_or_else(|e| panic!("could not create kube client config: {e}"));
                Client::try_from(client_config)
                    .unwrap_or_else(|e| panic!("could not create kube client: {e}"))
            }
            None => Client::try_default()
                .await
                .expect("could not create kube client"),
        };

        StacksDevnetApiK8sManager {
            client,
            ctx: ctx.to_owned(),
        }
    }

    pub async fn from_service<S, B, T>(
        service: S,
        default_namespace: T,
        ctx: &Context,
    ) -> StacksDevnetApiK8sManager
    where
        S: tower::Service<Request<Body>, Response = Response<B>> + Send + 'static,
        S::Future: Send + 'static,
        S::Error: Into<BoxError>,
        B: HttpBody<Data = Bytes> + Send + 'static,
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
        let namespace = &config.namespace;
        let user_id = &config.user_id;

        let context = format!("NAMESPACE: {}", &namespace);
        let namespace_exists = self.check_namespace_exists(namespace).await?;
        if !namespace_exists {
            if cfg!(debug_assertions) {
                self.deploy_namespace(namespace).await?;
            } else {
                let message =
                    format!("cannot create devnet because namespace {namespace} does not exist");
                self.ctx
                    .try_log(|logger| slog::warn!(logger, "{}", message));
                return Err(DevNetError { message, code: 400 });
            }
        }

        let any_assets_exist = self
            .check_any_devnet_assets_exist(namespace, user_id)
            .await?;
        if any_assets_exist {
            let message = format!("cannot create devnet because assets already exist {context}");
            self.ctx
                .try_log(|logger| slog::warn!(logger, "{}", message));
            return Err(DevNetError { message, code: 409 });
        };

        self.deploy_bitcoin_node(&config).await?;

        sleep(Duration::from_secs(5));

        self.deploy_stacks_blockchain(&config).await?;
        self.deploy_stacks_signer(
            &config,
            SignerIdx::Signer0,
            "7287ba251d44a4d3fd9276c88ce34c5c52a038955511cccaf77e61068649c17801",
        )
        .await?;

        self.deploy_stacks_signer(
            &config,
            SignerIdx::Signer1,
            "530d9f61984c888536871c6573073bdfc0058896dc1adfe9a6a10dfacadc209101",
        )
        .await?;

        if !config.disable_stacks_api {
            self.deploy_stacks_blockchain_api(&config).await?;
        }
        Ok(())
    }

    pub async fn delete_devnet(&self, namespace: &str, user_id: &str) -> Result<(), DevNetError> {
        match self
            .check_any_devnet_assets_exist(namespace, user_id)
            .await?
        {
            true => {
                let mut errors = vec![];
                let deployments: Vec<String> = StacksDevnetDeployment::iter()
                    .map(|p| p.to_string())
                    .collect();
                for deployment in deployments {
                    if let Err(e) = self
                        .delete_resource::<Deployment>(namespace, &deployment)
                        .await
                    {
                        errors.push(e);
                    }
                }

                let stateful_sets: Vec<String> = StacksDevnetStatefulSet::iter()
                    .map(|p| p.to_string())
                    .collect();

                for stateful_set in stateful_sets {
                    if let Err(e) = self
                        .delete_resource::<StatefulSet>(namespace, &stateful_set)
                        .await
                    {
                        errors.push(e);
                    }
                }

                let configmaps: Vec<String> = StacksDevnetConfigmap::iter()
                    .map(|c| c.to_string())
                    .collect();
                for configmap in configmaps {
                    if let Err(e) = self
                        .delete_resource::<ConfigMap>(namespace, &configmap)
                        .await
                    {
                        errors.push(e);
                    }
                }

                let services: Vec<String> =
                    StacksDevnetService::iter().map(|s| s.to_string()).collect();
                for service in services {
                    if let Err(e) = self.delete_resource::<Service>(namespace, &service).await {
                        errors.push(e);
                    }
                }

                let pvcs: Vec<String> =
                    StacksDevnetPvc::iter().map(|pvc| pvc.to_string()).collect();
                for pvc in pvcs {
                    if let Err(e) = self
                        .delete_resource_by_label::<PersistentVolumeClaim>(namespace, &pvc, user_id)
                        .await
                    {
                        errors.push(e);
                    }
                }

                if errors.is_empty() {
                    Ok(())
                } else if errors.len() == 1 {
                    match errors.first() {
                        Some(e) => Err(e.clone()),
                        None => unreachable!(),
                    }
                } else {
                    let mut msg = "multiple errors occurred while deleting devnet: ".to_string();
                    for e in errors {
                        msg = format!("{} \n- {}", msg, e.message);
                    }
                    Err(DevNetError {
                        message: msg,
                        code: 500,
                    })
                }
            }
            false => {
                let message = format!(
                    "cannot delete devnet because assets do not exist NAMESPACE: {}",
                    &namespace
                );
                self.ctx
                    .try_log(|logger| slog::warn!(logger, "{}", message));
                Err(DevNetError { message, code: 409 })
            }
        }
    }

    pub async fn check_namespace_exists(&self, namespace_str: &str) -> Result<bool, DevNetError> {
        self.ctx.try_log(|logger| {
            slog::info!(
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
                let message = format!("error getting namespace {namespace_str}: {e}");
                self.ctx
                    .try_log(|logger| slog::error!(logger, "{}", message));
                Err(DevNetError { message, code: 500 })
            }
        }
    }

    pub async fn check_any_devnet_assets_exist(
        &self,
        namespace: &str,
        user_id: &str,
    ) -> Result<bool, DevNetError> {
        self.ctx.try_log(|logger| {
            slog::info!(
                logger,
                "checking if any devnet assets exist for devnet NAMESPACE: {}",
                &namespace
            )
        });
        for deployment in StacksDevnetDeployment::iter() {
            if self
                .check_resource_exists::<Deployment>(namespace, &deployment.to_string())
                .await?
            {
                return Ok(true);
            }
        }

        for stateful_set in StacksDevnetStatefulSet::iter() {
            if self
                .check_resource_exists::<StatefulSet>(namespace, &stateful_set.to_string())
                .await?
            {
                return Ok(true);
            }
        }

        for pod in StacksDevnetPod::iter() {
            if self
                .check_resource_exists_by_label::<Pod>(namespace, &pod.to_string(), user_id)
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
                .check_resource_exists_by_label::<PersistentVolumeClaim>(
                    namespace,
                    &pvc.to_string(),
                    user_id,
                )
                .await?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn check_all_devnet_assets_exist(
        &self,
        namespace: &str,
    ) -> Result<bool, DevNetError> {
        self.ctx.try_log(|logger| {
            slog::info!(
                logger,
                "checking if all devnet assets exist for devnet NAMESPACE: {}",
                &namespace
            )
        });
        for deployment in StacksDevnetDeployment::iter() {
            if !self
                .check_resource_exists::<Deployment>(namespace, &deployment.to_string())
                .await?
            {
                return Ok(false);
            }
        }

        for stateful_set in StacksDevnetStatefulSet::iter() {
            if !self
                .check_resource_exists::<StatefulSet>(namespace, &stateful_set.to_string())
                .await?
            {
                return Ok(false);
            }
        }

        for configmap in StacksDevnetConfigmap::iter() {
            if !self
                .check_resource_exists::<ConfigMap>(namespace, &configmap.to_string())
                .await?
            {
                return Ok(false);
            }
        }

        for service in StacksDevnetService::iter() {
            if !self
                .check_resource_exists::<Service>(namespace, &service.to_string())
                .await?
            {
                return Ok(false);
            }
        }

        Ok(true)
    }

    async fn get_pod_status_info(
        &self,
        namespace: &str,
        user_id: &str,
        pod: StacksDevnetPod,
    ) -> Result<PodStatusResponse, DevNetError> {
        let context = format!("NAMESPACE: {namespace}, POD: {pod}");

        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
            slog::info!(logger, "getting pod status {}", context)
        });
        let pod_api: Api<Pod> = Api::namespaced(self.client.to_owned(), namespace);

        let pod_label_selector = format!("{COMPONENT_SELECTOR}={pod}");
        let user_label_selector = format!("{USER_SELECTOR}={user_id}");
        let name_label_selector = format!("{NAME_SELECTOR}={pod}");
        let label_selector =
            format!("{pod_label_selector},{user_label_selector},{name_label_selector}");

        let lp = ListParams::default()
            .match_any()
            .labels(&label_selector)
            .limit(1);

        match pod_api.list(&lp).await {
            Ok(pods) => {
                let pod_with_status = &pods.items[0];
                match &pod_with_status.status {
                    Some(status) => {
                        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
                            slog::info!(logger, "successfully retrieved pod status {}", context)
                        });
                        let start_time = status.start_time.as_ref().map(|st| st.0.to_string());
                        Ok(PodStatusResponse {
                            status: status.phase.to_owned(),
                            start_time,
                        })
                    }
                    None => Ok(PodStatusResponse::default()),
                }
            }
            Err(e) => {
                let (msg, code) = match e {
                    kube::Error::Api(api_error) => (api_error.message, api_error.code),
                    e => (e.to_string(), 500),
                };
                let msg = format!("failed to get pod status {context}, ERROR: {msg}");
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError { message: msg, code })
            }
        }
    }

    async fn get_stacks_v2_info(
        &self,
        namespace: &str,
    ) -> Result<StacksV2InfoResponse, DevNetError> {
        let client = HttpClient::new();

        let url = get_service_url(namespace, StacksDevnetService::StacksBlockchain);
        let port =
            get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::RPC).unwrap();
        let url = format!("http://{url}:{port}/v2/info");

        let context = format!("NAMESPACE: {namespace}");

        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
            slog::info!(logger, "Requesting URL: {}", url); // Log the full URL
        });

        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
            slog::info!(
                logger,
                "requesting /v2/info route of stacks node {}",
                context
            );
        });

        match Uri::from_str(&url) {
            Ok(uri) => {
                match client.get(uri).await {
                    Ok(response) => match hyper::body::to_bytes(response.into_body()).await {
                        Ok(body) => {
                            let body_str = String::from_utf8_lossy(&body);
                            self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
                                slog::info!(logger, "Raw response body: {}", body_str);
                            });

                            match serde_json::from_slice::<StacksV2InfoResponse>(&body) {
                                Ok(config) => {
                                    self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
                                        slog::info!(logger, "successfully requested /v2/info route of stacks node {}", context);
                                    });
                                    Ok(config)
                                }
                                Err(e) => {
                                    let msg = format!("failed to parse JSON response: {context}, ERROR: {e}, Raw body: {body_str}");
                                    self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                                    Err(DevNetError {
                                        message: msg,
                                        code: 500,
                                    })
                                }
                            }
                        }
                        Err(e) => {
                            let msg =
                                format!("failed to parse response bytes: {context}, ERROR: {e}");
                            self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                            Err(DevNetError {
                                message: msg,
                                code: 500,
                            })
                        }
                    },
                    Err(e) => {
                        let msg = format!("failed to query stacks node: {context}, ERROR: {e}");
                        self.ctx.try_log(|logger| slog::warn!(logger, "{}", msg));
                        Ok(StacksV2InfoResponse::default()) // Return default response on error
                    }
                }
            }
            Err(e) => {
                let msg = format!("failed to parse url: {url} ERROR: {e}");
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
        user_id: &str,
    ) -> Result<StacksDevnetInfoResponse, DevNetError> {
        let context = format!("NAMESPACE: {namespace}");

        match self.check_all_devnet_assets_exist(namespace).await? {
            false => {
                let msg = format!("not all devnet assets exist {context}");
                self.ctx
                    .try_log(|logger: &hiro_system_kit::Logger| slog::info!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: 404,
                })
            }
            true => {
                self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
                    slog::info!(logger, "getting devnet info {}", context)
                });

                // Fetch the pod status information
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
                ) = try_join3(
                    self.get_pod_status_info(namespace, user_id, StacksDevnetPod::BitcoindNode),
                    self.get_pod_status_info(namespace, user_id, StacksDevnetPod::StacksBlockchain),
                    self.get_pod_status_info(
                        namespace,
                        user_id,
                        StacksDevnetPod::StacksBlockchainApi,
                    ),
                )
                .await?;

                // Try to fetch chain info, but handle errors by using default values for the chain tips
                let chain_info = match self.get_stacks_v2_info(namespace).await {
                    Ok(info) => info,
                    Err(e) => {
                        self.ctx.try_log(|logger: &hiro_system_kit::Logger| {
                            slog::warn!(logger, "Failed to get chain info: {}", e.message);
                        });
                        StacksV2InfoResponse {
                            stacks_tip_height: 0,
                            burn_block_height: 0,
                        }
                    }
                };

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
        }
    }

    pub async fn deploy_namespace(&self, namespace_str: &str) -> Result<(), DevNetError> {
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

    async fn get_resource_by_label<K>(
        &self,
        namespace: &str,
        name: &str,
        user_id: &str,
    ) -> Result<Option<K>, DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: Serialize,
        K: kube::Resource<Scope = NamespaceResourceScope>,
    {
        let resource_api: Api<K> = Api::namespaced(self.client.to_owned(), namespace);

        let pod_label_selector = format!("{COMPONENT_SELECTOR}={name}");
        let user_label_selector = format!("{USER_SELECTOR}={user_id}");
        let name_label_selector = format!("{NAME_SELECTOR}={name}");
        let label_selector =
            format!("{pod_label_selector},{user_label_selector},{name_label_selector}");
        let lp = ListParams::default()
            .match_any()
            .labels(&label_selector)
            .limit(1);

        let resource_details = format!(
            "RESOURCE: {}, NAME: {}, NAMESPACE: {}",
            std::any::type_name::<K>(),
            name,
            namespace
        );
        self.ctx
            .try_log(|logger| slog::info!(logger, "fetching {}", resource_details));

        match resource_api.list(&lp).await {
            Ok(pods) => {
                if !pods.items.is_empty() {
                    let pod = &pods.items[0];
                    Ok(Some(pod.clone()))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                let (msg, code) = match e {
                    kube::Error::Api(api_error) => {
                        if api_error.code == 404 {
                            return Ok(None);
                        }
                        (api_error.message, api_error.code)
                    }
                    e => (e.to_string(), 500),
                };
                let message = format!("failed to fetch {resource_details}, ERROR: {msg}");
                self.ctx
                    .try_log(|logger| slog::error!(logger, "{}", message));
                Err(DevNetError { message, code })
            }
        }
    }

    async fn get_resource<K>(&self, namespace: &str, name: &str) -> Result<Option<K>, DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: Serialize,
        K: kube::Resource<Scope = NamespaceResourceScope>,
    {
        let resource_api: Api<K> = Api::namespaced(self.client.to_owned(), namespace);

        let resource_details = format!(
            "RESOURCE: {}, NAME: {}, NAMESPACE: {}",
            std::any::type_name::<K>(),
            name,
            namespace
        );
        self.ctx
            .try_log(|logger| slog::info!(logger, "fetching {}", resource_details));

        match resource_api.get_opt(name).await {
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
                let message = format!("failed to fetch {resource_details}, ERROR: {msg}");
                self.ctx
                    .try_log(|logger| slog::error!(logger, "{}", message));
                Err(DevNetError { message, code })
            }
        }
    }

    async fn check_resource_exists_by_label<K>(
        &self,
        namespace: &str,
        name: &str,
        user_id: &str,
    ) -> Result<bool, DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: Serialize,
        K: kube::Resource<Scope = NamespaceResourceScope>,
    {
        match self
            .get_resource_by_label::<K>(namespace, name, user_id)
            .await?
        {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    async fn check_resource_exists<K>(
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
        K: kube::Resource<Scope = NamespaceResourceScope>,
    {
        match self.get_resource::<K>(namespace, name).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    async fn deploy_resource<K>(
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
        K: kube::Resource<Scope = NamespaceResourceScope>,
    {
        let resource_api: Api<K> = Api::namespaced(self.client.to_owned(), namespace);
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
        let resource_details =
            format!("RESOURCE: {resource_type}, NAME: {name}, NAMESPACE: {namespace}");
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

    async fn deploy_deployment(
        &self,
        deployment_type: StacksDevnetDeployment,
        namespace: &str,
        user_id: &str,
    ) -> Result<(), DevNetError> {
        let deployment_type_moved = deployment_type.clone();
        let mut deployment: Deployment =
            self.get_resource_from_file(StacksDevnetResource::Deployment(deployment_type_moved))?;

        let key = "app.kubernetes.io/instance".to_string();
        let user_id = user_id.to_owned();

        // set deployment metadata labels to include user id
        if let Some(mut labels) = deployment.clone().metadata.labels {
            if let Some(label) = labels.get_mut(&key) {
                *label = user_id.clone();
            } else {
                labels.insert(key.clone(), user_id.clone());
            }
            deployment.metadata.labels = Some(labels);
        }

        // set deployment spec with user-specific data
        if let Some(mut spec) = deployment.clone().spec {
            if let Some(mut match_labels) = spec.selector.match_labels {
                if let Some(match_label) = match_labels.get_mut(&key) {
                    *match_label = user_id.clone();
                } else {
                    match_labels.insert(key.clone(), user_id.clone());
                }
                spec.selector.match_labels = Some(match_labels);
            }

            let mut template = spec.template;
            if let Some(mut metadata) = template.metadata {
                if let Some(mut labels) = metadata.labels {
                    if let Some(label) = labels.get_mut(&key) {
                        *label = user_id.clone();
                    } else {
                        labels.insert(key.clone(), user_id.clone());
                    }
                    metadata.labels = Some(labels);
                }
                template.metadata = Some(metadata);
            }

            spec.template = template;

            deployment.spec = Some(spec);
        }

        deployment.metadata.namespace = Some(namespace.to_owned());
        self.deploy_resource(namespace, deployment, "deployment")
            .await
    }

    async fn deploy_stateful_set(
        &self,
        stateful_set_type: StacksDevnetStatefulSet,
        namespace: &str,
        user_id: &str,
    ) -> Result<(), DevNetError> {
        let stateful_set_type_moved = stateful_set_type.clone();
        let mut stateful_set: StatefulSet = self
            .get_resource_from_file(StacksDevnetResource::StatefulSet(stateful_set_type_moved))?;
        let key = "app.kubernetes.io/instance".to_string();
        let user_id = user_id.to_owned();

        if let Some(mut labels) = stateful_set.clone().metadata.labels {
            if let Some(label) = labels.get_mut(&key) {
                *label = user_id.clone();
            } else {
                labels.insert(key.clone(), user_id.clone());
            }
            stateful_set.metadata.labels = Some(labels);
        }

        if let Some(mut spec) = stateful_set.clone().spec {
            if let Some(mut match_labels) = spec.selector.match_labels {
                if let Some(match_label) = match_labels.get_mut(&key) {
                    *match_label = user_id.clone();
                } else {
                    match_labels.insert(key.clone(), user_id.clone());
                }
                spec.selector.match_labels = Some(match_labels);
            }

            let mut template = spec.template;
            if let Some(mut metadata) = template.metadata {
                if let Some(mut labels) = metadata.labels {
                    if let Some(label) = labels.get_mut(&key) {
                        *label = user_id.clone();
                    } else {
                        labels.insert(key.clone(), user_id.clone());
                    }
                    metadata.labels = Some(labels);
                }
                template.metadata = Some(metadata);
            }

            spec.template = template;
            stateful_set.spec = Some(spec);
        }

        stateful_set.metadata.namespace = Some(namespace.to_owned());
        self.deploy_resource(namespace, stateful_set, "stateful_set")
            .await
    }

    async fn deploy_service(
        &self,
        service: StacksDevnetService,
        namespace: &str,
        user_id: &str,
    ) -> Result<(), DevNetError> {
        let mut service: Service =
            self.get_resource_from_file(StacksDevnetResource::Service(service))?;

        let key = "app.kubernetes.io/instance".to_string();
        let user_id = user_id.to_owned();

        if let Some(mut labels) = service.clone().metadata.labels {
            if let Some(label) = labels.get_mut(&key) {
                *label = user_id.clone();
            } else {
                labels.insert(key.clone(), user_id.clone());
            }
            service.metadata.labels = Some(labels);
        }

        if let Some(mut spec) = service.clone().spec {
            if let Some(mut selector_map) = spec.selector {
                if let Some(selector_entry) = selector_map.get_mut(&key) {
                    *selector_entry = user_id.clone();
                } else {
                    selector_map.insert(key.clone(), user_id.clone());
                }
                spec.selector = Some(selector_map);
            }

            service.spec = Some(spec);
        }
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
                map.insert(key, value);
            }
            configmap.data = Some(map);
        }

        self.deploy_resource(namespace, configmap, "configmap")
            .await
    }

    async fn deploy_bitcoin_node(
        &self,
        config: &ValidatedStacksDevnetConfig,
    ) -> Result<(), DevNetError> {
        let namespace = &config.namespace;
        let user_id = &config.user_id;
        let devnet_config = &config.devnet_config;

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
            devnet_config.bitcoin_node_username,
            devnet_config.bitcoin_node_password,
            bitcoin_p2p_port,
            bitcoin_rpc_port,
            bitcoin_rpc_port
        );

        self.deploy_configmap(
            StacksDevnetConfigmap::BitcoindNode,
            namespace,
            Some(vec![("bitcoin.conf".into(), bitcoind_conf)]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::ProjectManifest,
            namespace,
            Some(vec![(
                "Clarinet.toml".into(),
                config.project_manifest_yaml_string.to_owned(),
            )]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::Devnet,
            namespace,
            Some(vec![(
                "Devnet.toml".into(),
                config.network_manifest_yaml_string.to_owned(),
            )]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::DeploymentPlan,
            namespace,
            Some(vec![(
                "default.devnet-plan.yaml".into(),
                config.deployment_plan_yaml_string.to_owned(),
            )]),
        )
        .await?;

        self.deploy_configmap(
            StacksDevnetConfigmap::ProjectDir,
            namespace,
            Some(config.contract_configmap_data.to_owned()),
        )
        .await?;

        self.deploy_deployment(StacksDevnetDeployment::BitcoindNode, namespace, user_id)
            .await?;

        self.deploy_service(StacksDevnetService::BitcoindNode, namespace, user_id)
            .await?;

        Ok(())
    }

    async fn deploy_stacks_blockchain(
        &self,
        config: &ValidatedStacksDevnetConfig,
    ) -> Result<(), DevNetError> {
        let namespace = &config.namespace;
        let user_id = &config.user_id;
        let devnet_config = &config.devnet_config;

        let chain_coordinator_ingestion_port =
            get_service_port(StacksDevnetService::BitcoindNode, ServicePort::Ingestion).unwrap();

        let (miner_coinbase_recipient, _, stacks_miner_secret_key_hex) = compute_addresses(
            &devnet_config.miner_mnemonic,
            &devnet_config.miner_derivation_path,
            &StacksNetwork::Devnet.get_networks(),
        );

        let stacks_conf = {
            let mut stacks_conf = format!(
                r#"
                    [node]
                    working_dir = "/devnet"
                    rpc_bind = "0.0.0.0:{}"
                    p2p_bind = "0.0.0.0:{}"
                    data_url = "http://127.0.0.1:{}"
                    p2p_address = "127.0.0.1:{}"
                    miner = true
                    stacker = true
                    seed = "{}"
                    local_peer_seed = "{}"
                    pox_sync_sample_secs = 0
                    wait_time_for_blocks = 0
                    wait_time_for_microblocks = 0
                    next_initiative_delay = 4000
                    mine_microblocks = false
                    microblock_frequency = 1000

                    [connection_options]
                    # inv_sync_interval = 10
                    # download_interval = 10
                    # walk_interval = 10
                    disable_block_download = true
                    disable_inbound_handshakes = true
                    disable_inbound_walks = true
                    public_ip_address = "1.1.1.1:1234"
                    auth_token = "12345"

                    [miner]
                    first_attempt_time_ms = {}
                    block_reward_recipient = "{}"
                    microblock_attempt_time_ms = 10
                    mining_key = "19ec1c3e31d139c989a23a27eac60d1abfad5277d3ae9604242514c738258efa01"
                "#,
                get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::RPC).unwrap(),
                get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::P2P).unwrap(),
                get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::RPC).unwrap(),
                get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::P2P).unwrap(),
                stacks_miner_secret_key_hex,
                stacks_miner_secret_key_hex,
                devnet_config.stacks_node_first_attempt_time_ms,
                miner_coinbase_recipient
            );

            for (_, account) in config.accounts.iter() {
                stacks_conf.push_str(&format!(
                    r#"
                    [[ustx_balance]]
                    address = "{}"
                    amount = {}
                "#,
                    account.stx_address, account.balance
                ));
            }

            let balance: u64 = 100_000_000_000_000;
            stacks_conf.push_str(&format!(
                r#"
                [[ustx_balance]]
                address = "{miner_coinbase_recipient}"
                amount = {balance}
                "#
            ));

            let bitcoind_chain_coordinator_host =
                get_service_url(namespace, StacksDevnetService::BitcoindNode);

            stacks_conf.push_str(&format!(
                r#"
                # Add orchestrator (docker-host) as an event observer
                [[events_observer]]
                endpoint = "{bitcoind_chain_coordinator_host}:{chain_coordinator_ingestion_port}"
                events_keys = ["*"]
                "#
            ));

            stacks_conf.push_str(&format!(
                r#"
            # Add stacks-blockchain-api as an event observer
            [[events_observer]]
            endpoint = "{}:{}"
            events_keys = ["*"]
            "#,
                get_service_url(namespace, StacksDevnetService::StacksBlockchainApi),
                get_service_port(StacksDevnetService::StacksBlockchainApi, ServicePort::Event)
                    .unwrap(),
            ));

            for signer_idx in SignerIdx::iter() {
                let (url, port) = match signer_idx {
                    SignerIdx::Signer0 => (
                        get_service_url(namespace, StacksDevnetService::StacksSigner0),
                        get_service_port(StacksDevnetService::StacksSigner0, ServicePort::Event)
                            .unwrap(),
                    ),
                    SignerIdx::Signer1 => (
                        get_service_url(namespace, StacksDevnetService::StacksSigner1),
                        get_service_port(StacksDevnetService::StacksSigner1, ServicePort::Event)
                            .unwrap(),
                    ),
                };

                stacks_conf.push_str(&format!(
                    r#"
                # Add stacks-signer-{signer_idx} as an event observer
                [[events_observer]]
                endpoint = "{url}:{port}"
                events_keys = ["stackerdb", "block_proposal", "burn_blocks"]
                "#,
                ));
            }

            stacks_conf.push_str(&format!(
                r#"
                [burnchain]
                chain = "bitcoin"
                mode = "nakamoto-neon"
                magic_bytes = "T3"
                first_burn_block_height = 100
                pox_prepare_length = 5
                pox_reward_length = 20
                burn_fee_cap = 20_000
                poll_time_secs = 1
                timeout = 30
                peer_host = "{}"
                rpc_ssl = false
                wallet_name = "{}"
                username = "{}"
                password = "{}"
                rpc_port = {}
                peer_port = {}
                "#,
                bitcoind_chain_coordinator_host,
                devnet_config.miner_wallet_name,
                devnet_config.bitcoin_node_username,
                devnet_config.bitcoin_node_password,
                chain_coordinator_ingestion_port,
                get_service_port(StacksDevnetService::BitcoindNode, ServicePort::P2P).unwrap()
            ));

            stacks_conf.push_str(
                r#"
                [[burnchain.epochs]]
                epoch_name = "1.0"
                start_height = 0
                "#,
            );
            let epoch_conf = BurnchainEpochConfig::from(devnet_config);
            let epoch_conf_str = toml::to_string(&epoch_conf).map_err(|e| DevNetError {
                message: format!("failed to serialize epoch config: {e}"),
                code: 500,
            })?;
            stacks_conf.push_str(&epoch_conf_str);

            stacks_conf
        };

        self.deploy_configmap(
            StacksDevnetConfigmap::StacksBlockchain,
            namespace,
            Some(vec![("Stacks.toml".into(), stacks_conf)]),
        )
        .await?;

        self.deploy_deployment(StacksDevnetDeployment::StacksBlockchain, namespace, user_id)
            .await?;

        self.deploy_service(StacksDevnetService::StacksBlockchain, namespace, user_id)
            .await?;

        Ok(())
    }

    async fn deploy_stacks_blockchain_api(
        &self,
        config: &ValidatedStacksDevnetConfig,
    ) -> Result<(), DevNetError> {
        let namespace = &config.namespace;
        let user_id = &config.user_id;
        // configmap env vars for pg conatainer
        let stacks_api_pg_env = Vec::from([
            ("POSTGRES_PASSWORD".into(), "postgres".into()),
            ("POSTGRES_DB".into(), "stacks_api".into()),
        ]);
        self.deploy_configmap(
            StacksDevnetConfigmap::StacksBlockchainApiPg,
            namespace,
            Some(stacks_api_pg_env),
        )
        .await?;

        // configmap env vars for api conatainer
        let stacks_node_host = get_service_url(namespace, StacksDevnetService::StacksBlockchain);
        let rpc_port =
            get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::RPC).unwrap();
        let api_port =
            get_service_port(StacksDevnetService::StacksBlockchainApi, ServicePort::API).unwrap();
        let event_port =
            get_service_port(StacksDevnetService::StacksBlockchainApi, ServicePort::Event).unwrap();
        let db_port =
            get_service_port(StacksDevnetService::StacksBlockchainApi, ServicePort::DB).unwrap();
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
            (
                "FAUCET_PRIVATE_KEY".into(),
                config.devnet_config.faucet_secret_key_hex.clone(),
            ),
        ]);
        self.deploy_configmap(
            StacksDevnetConfigmap::StacksBlockchainApi,
            namespace,
            Some(stacks_api_env),
        )
        .await?;

        self.deploy_stateful_set(
            StacksDevnetStatefulSet::StacksBlockchainApi,
            namespace,
            user_id,
        )
        .await?;

        self.deploy_service(StacksDevnetService::StacksBlockchainApi, namespace, user_id)
            .await?;

        Ok(())
    }

    async fn deploy_stacks_signer(
        &self,
        config: &ValidatedStacksDevnetConfig,
        signer_idx: SignerIdx,
        signer_key: &str,
    ) -> Result<(), DevNetError> {
        let namespace = &config.namespace;
        let user_id = &config.user_id;

        let signer_port = match signer_idx {
            SignerIdx::Signer0 => {
                get_service_port(StacksDevnetService::StacksSigner0, ServicePort::Event).unwrap()
            }
            SignerIdx::Signer1 => {
                get_service_port(StacksDevnetService::StacksSigner1, ServicePort::Event).unwrap()
            }
        };

        let configmap = match signer_idx {
            SignerIdx::Signer0 => StacksDevnetConfigmap::StacksSigner0,
            SignerIdx::Signer1 => StacksDevnetConfigmap::StacksSigner1,
        };

        let sts = match signer_idx {
            SignerIdx::Signer0 => StacksDevnetStatefulSet::StacksSigner0,
            SignerIdx::Signer1 => StacksDevnetStatefulSet::StacksSigner1,
        };

        let service = match signer_idx {
            SignerIdx::Signer0 => StacksDevnetService::StacksSigner0,
            SignerIdx::Signer1 => StacksDevnetService::StacksSigner1,
        };

        // configmap env vars for api conatainer
        let signer_conf = format!(
            r#"
                    stacks_private_key = "{}"
                    node_host = "{}:{}"
                    # must be added as event_observer in node config:
                    endpoint =  "0.0.0.0:{}"
                    network = "testnet"
                    auth_password = "12345"
                    db_path = "/chainstate/stacks-signer-{}.sqlite"
                "#,
            signer_key,
            get_service_url(namespace, StacksDevnetService::StacksBlockchain),
            get_service_port(StacksDevnetService::StacksBlockchain, ServicePort::RPC).unwrap(),
            signer_port,
            signer_idx
        );

        self.deploy_configmap(
            configmap,
            namespace,
            Some(vec![("Signer.toml".into(), signer_conf)]),
        )
        .await?;

        self.deploy_stateful_set(sts, namespace, user_id).await?;

        self.deploy_service(service, namespace, user_id).await?;

        Ok(())
    }

    async fn delete_resource<K>(
        &self,
        namespace: &str,
        resource_name: &str,
    ) -> Result<(), DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: kube::Resource<Scope = NamespaceResourceScope>,
    {
        let api: Api<K> = Api::namespaced(self.client.to_owned(), namespace);
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

    async fn delete_resource_by_label<K>(
        &self,
        namespace: &str,
        resource_name: &str,
        user_id: &str,
    ) -> Result<(), DevNetError>
    where
        <K as kube::Resource>::DynamicType: Default,
        K: Clone,
        K: DeserializeOwned,
        K: std::fmt::Debug,
        K: kube::Resource<Scope = NamespaceResourceScope>,
    {
        let api: Api<K> = Api::namespaced(self.client.to_owned(), namespace);
        let dp = DeleteParams::default();

        let pod_label_selector = format!("{COMPONENT_SELECTOR}={resource_name}");
        let user_label_selector = format!("{USER_SELECTOR}={user_id}");
        let name_label_selector = format!("{NAME_SELECTOR}={resource_name}");
        let label_selector =
            format!("{pod_label_selector},{user_label_selector},{name_label_selector}");

        let lp = ListParams::default()
            .match_any()
            .labels(&label_selector)
            .limit(1);

        let resource_details = format!(
            "RESOURCE: {}, NAME: {}, NAMESPACE: {}",
            std::any::type_name::<K>(),
            resource_name,
            namespace
        );
        self.ctx
            .try_log(|logger| slog::info!(logger, "deleting {}", resource_details));
        match api.delete_collection(&dp, &lp).await {
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

    pub async fn delete_namespace(&self, namespace_str: &str) -> Result<(), DevNetError> {
        if cfg!(debug_assertions) {
            use kube::ResourceExt;
            let api: Api<Namespace> = kube::Api::all(self.client.to_owned());

            let dp = DeleteParams::default();
            match api.delete(namespace_str, &dp).await {
                Ok(namespace) => {
                    namespace.map_left(|del| {
                        assert_eq!(del.name_any(), namespace_str);
                        self.ctx
                            .try_log(|logger| slog::info!(logger, "Deleting namespace started"));
                    });
                    Ok(())
                }
                Err(kube::Error::Api(api_error)) => Err(DevNetError {
                    message: format!("unable to delete namespace: {}", api_error.message),
                    code: api_error.code,
                }),
                Err(e) => Err(DevNetError {
                    message: format!("unable to delete namespace: {e}"),
                    code: 500,
                }),
            }
        } else {
            Err(DevNetError {
                message: "namespace deletion can only occur in debug mode".to_string(),
                code: 403,
            })
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
                let msg = format!("unable to parse template file: {e}");
                self.ctx.try_log(|logger| slog::error!(logger, "{}", msg));
                Err(DevNetError {
                    message: msg,
                    code: 500,
                })
            }
        }
    }
}
