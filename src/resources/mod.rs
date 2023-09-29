use self::{
    configmap::StacksDevnetConfigmap, deployment::StacksDevnetDeployment, pod::StacksDevnetPod,
    service::StacksDevnetService, stateful_set::StacksDevnetStatefulSet,
};

pub mod configmap;
pub mod deployment;
pub mod pod;
pub mod service;
pub mod stateful_set;

pub enum StacksDevnetResource {
    Configmap(StacksDevnetConfigmap),
    Deployment(StacksDevnetDeployment),
    Pod(StacksDevnetPod),
    Service(StacksDevnetService),
    StatefulSet(StacksDevnetStatefulSet),
    Namespace,
}

#[cfg(test)]
pub mod tests;
