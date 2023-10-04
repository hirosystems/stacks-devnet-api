use self::{
    configmap::StacksDevnetConfigmap, deployment::StacksDevnetDeployment, pod::StacksDevnetPod,
    pvc::StacksDevnetPvc, service::StacksDevnetService, stateful_set::StacksDevnetStatefulSet,
};

pub mod configmap;
pub mod deployment;
pub mod pod;
pub mod pvc;
pub mod service;
pub mod stateful_set;

pub enum StacksDevnetResource {
    Configmap(StacksDevnetConfigmap),
    Deployment(StacksDevnetDeployment),
    Pod(StacksDevnetPod),
    Pvc(StacksDevnetPvc),
    Service(StacksDevnetService),
    StatefulSet(StacksDevnetStatefulSet),
    Namespace,
}

#[cfg(test)]
pub mod tests;
