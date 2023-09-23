use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetPvc {
    StacksApi,
    StacksNode,
    BitcoindNode,
}

impl fmt::Display for StacksDevnetPvc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetPvc::StacksApi => write!(f, "stacks-api-pvc"),
            StacksDevnetPvc::StacksNode => write!(f, "stacks-node-pvc"),
            StacksDevnetPvc::BitcoindNode => write!(f, "bitcoind-node-pvc"),
        }
    }
}
