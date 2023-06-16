use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetPod {
    BitcoindNode,
    StacksNode,
    StacksApi,
}

impl fmt::Display for StacksDevnetPod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetPod::BitcoindNode => write!(f, "bitcoind-chain-coordinator"),
            StacksDevnetPod::StacksNode => write!(f, "stacks-node"),
            StacksDevnetPod::StacksApi => write!(f, "stacks-api"),
        }
    }
}
