use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetConfigmap {
    BitcoindNode,
    StacksNode,
    StacksApi,
    StacksApiPostgres,
    DeploymentPlan,
    Devnet,
    ProjectDir,
    Namespace,
    ProjectManifest,
}

impl fmt::Display for StacksDevnetConfigmap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetConfigmap::BitcoindNode => write!(f, "bitcoind-conf"),
            StacksDevnetConfigmap::StacksNode => write!(f, "stacks-node-conf"),
            StacksDevnetConfigmap::StacksApi => write!(f, "stacks-api-conf"),
            StacksDevnetConfigmap::StacksApiPostgres => write!(f, "stacks-api-postgres-conf"),
            StacksDevnetConfigmap::DeploymentPlan => write!(f, "deployment-plan-conf"),
            StacksDevnetConfigmap::Devnet => write!(f, "devnet-conf"),
            StacksDevnetConfigmap::ProjectDir => write!(f, "project-dir-conf"),
            StacksDevnetConfigmap::Namespace => write!(f, "namespace-conf"),
            StacksDevnetConfigmap::ProjectManifest => write!(f, "project-manifest-conf"),
        }
    }
}
