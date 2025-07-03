use std::fmt;
use strum::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetPod {
    BitcoindNode,
    StacksBlockchain,
    StacksBlockchainApi,
    StacksSigner0,
}

impl fmt::Display for StacksDevnetPod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetPod::BitcoindNode => write!(f, "bitcoind-chain-coordinator"),
            StacksDevnetPod::StacksBlockchain => write!(f, "stacks-blockchain"),
            StacksDevnetPod::StacksBlockchainApi => write!(f, "stacks-blockchain-api"),
            StacksDevnetPod::StacksSigner0 => write!(f, "stacks-signer-0"),
        }
    }
}
