use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetPvc {
    StacksBlockchainApiPg,
}

impl fmt::Display for StacksDevnetPvc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetPvc::StacksBlockchainApiPg => write!(f, "stacks-blockchain-api-pg"),
        }
    }
}
