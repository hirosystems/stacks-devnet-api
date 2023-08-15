use self::{
    configmap::StacksDevnetConfigmap, pod::StacksDevnetPod, pvc::StacksDevnetPvc,
    service::StacksDevnetService,
};

pub mod configmap;
pub mod pod;
pub mod pvc;
pub mod service;

pub enum StacksDevnetResource {
    Configmap(StacksDevnetConfigmap),
    Pod(StacksDevnetPod),
    Pvc(StacksDevnetPvc),
    Service(StacksDevnetService),
    Namespace,
}

#[cfg(test)]
pub mod tests;
