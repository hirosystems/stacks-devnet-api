use std::fmt;
use strum_macros::EnumIter;

#[derive(EnumIter, Debug)]
pub enum StacksDevnetStatefulSet {
    StacksBlockchainApi,
}

impl fmt::Display for StacksDevnetStatefulSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            StacksDevnetStatefulSet::StacksBlockchainApi => write!(f, "stacks-blockchain-api"),
        }
    }
}
