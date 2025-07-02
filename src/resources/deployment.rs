use std::fmt;
use strum::EnumIter;

#[derive(EnumIter, Debug, Clone)]
pub enum StacksDevnetDeployment {
    BitcoindNode,
    StacksBlockchain,
}

impl fmt::Display for StacksDevnetDeployment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetDeployment::BitcoindNode => write!(f, "bitcoind-chain-coordinator"),
            StacksDevnetDeployment::StacksBlockchain => write!(f, "stacks-blockchain"),
        }
    }
}
