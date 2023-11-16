use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetPod {
    BitcoindNode,
    StacksBlockchain,
    StacksBlockchainApi,
}

impl fmt::Display for StacksDevnetPod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetPod::BitcoindNode => write!(f, "bitcoind-chain-coordinator"),
            StacksDevnetPod::StacksBlockchain => write!(f, "stacks-blockchain"),
            StacksDevnetPod::StacksBlockchainApi => write!(f, "stacks-blockchain-api"),
        }
    }
}
