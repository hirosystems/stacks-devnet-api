use std::fmt;
use strum::EnumIter;

#[derive(EnumIter, Debug, Clone)]
pub enum StacksDevnetStatefulSet {
    StacksBlockchainApi,
    StacksSigner0,
    StacksSigner1,
}

impl fmt::Display for StacksDevnetStatefulSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetStatefulSet::StacksBlockchainApi => write!(f, "stacks-blockchain-api"),
            StacksDevnetStatefulSet::StacksSigner0 => write!(f, "stacks-signer-0"),
            StacksDevnetStatefulSet::StacksSigner1 => write!(f, "stacks-signer-1"),
        }
    }
}

#[derive(EnumIter, Debug, Clone)]
pub enum SignerIdx {
    Signer0,
    Signer1,
}

impl fmt::Display for SignerIdx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignerIdx::Signer0 => write!(f, "0"),
            SignerIdx::Signer1 => write!(f, "1"),
        }
    }
}
