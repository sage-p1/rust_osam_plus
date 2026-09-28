use crate::Address;
use std::{error::Error, fmt};

/// Errors raised when a client violates the SAM contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SamError {
    /// An address was never allocated or was already consumed.
    InvalidAddress(Address),
    /// An address exceeded its configured read allowance.
    ReadLimitExceeded { address: Address, limit: usize },
    /// An address exceeded its configured write allowance.
    WriteLimitExceeded { address: Address, limit: usize },
    /// A pointer encountered a cell of the wrong kind.
    InvalidPointerCell(&'static str),
    /// A caller supplied an invalid parameter.
    InvalidParameter(&'static str),
    /// The cryptographic backend or fixed-block codec rejected an operation.
    Backend(String),
}

impl fmt::Display for SamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAddress(address) => write!(formatter, "invalid SAM address {address:?}"),
            Self::ReadLimitExceeded { address, limit } => {
                write!(formatter, "address {address:?} exceeds read limit {limit}")
            }
            Self::WriteLimitExceeded { address, limit } => {
                write!(formatter, "address {address:?} exceeds write limit {limit}")
            }
            Self::InvalidPointerCell(message) => {
                write!(formatter, "invalid pointer cell: {message}")
            }
            Self::InvalidParameter(message) => write!(formatter, "invalid parameter: {message}"),
            Self::Backend(message) => write!(formatter, "SAM backend error: {message}"),
        }
    }
}

impl Error for SamError {}
