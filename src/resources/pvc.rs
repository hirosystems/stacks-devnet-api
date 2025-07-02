use std::fmt;
use strum::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetPvc {
    StacksBlockchainApiPg,
    StacksSigner0,
    StacksSigner1,
}

impl fmt::Display for StacksDevnetPvc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetPvc::StacksBlockchainApiPg => write!(f, "stacks-blockchain-api"),
            StacksDevnetPvc::StacksSigner0 => write!(f, "stacks-signer-0"),
            StacksDevnetPvc::StacksSigner1 => write!(f, "stacks-signer-1"),
        }
    }
}
