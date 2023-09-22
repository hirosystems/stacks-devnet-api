use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetConfigmap {
    BitcoindNode,
    StacksBlockchain,
    StacksBlockchainApi,
    StacksBlockchainApiPg,
    DeploymentPlan,
    Devnet,
    ProjectDir,
    ProjectManifest,
}

impl fmt::Display for StacksDevnetConfigmap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetConfigmap::BitcoindNode => write!(f, "bitcoind"),
            StacksDevnetConfigmap::StacksBlockchain => write!(f, "stacks-blockchain"),
            StacksDevnetConfigmap::StacksBlockchainApi => write!(f, "stacks-blockchain-api"),
            StacksDevnetConfigmap::StacksBlockchainApiPg => write!(f, "stacks-blockchain-api-pg"),
            StacksDevnetConfigmap::DeploymentPlan => write!(f, "deployment-plan"),
            StacksDevnetConfigmap::Devnet => write!(f, "devnet"),
            StacksDevnetConfigmap::ProjectDir => write!(f, "project-dir"),
            StacksDevnetConfigmap::ProjectManifest => write!(f, "project-manifest"),
        }
    }
}
