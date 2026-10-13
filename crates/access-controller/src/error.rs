use std::{collections::TryReserveError, error::Error as StdError, fmt};

/// Technical failures are separate from an access denial.
#[derive(Debug)]
pub enum Error {
    Registry(policy_registry::Error),
    CompilationLimit(&'static str),
    Allocation(TryReserveError),
    Internal(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(error) => write!(formatter, "policy loading failed: {error}"),
            Self::CompilationLimit(limit) => {
                write!(formatter, "policy compilation exceeds the {limit} limit")
            }
            Self::Allocation(error) => write!(formatter, "policy index allocation failed: {error}"),
            Self::Internal(reason) => write!(formatter, "invalid policy index: {reason}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Registry(error) => Some(error),
            Self::Allocation(error) => Some(error),
            Self::CompilationLimit(_) | Self::Internal(_) => None,
        }
    }
}

impl From<policy_registry::Error> for Error {
    fn from(error: policy_registry::Error) -> Self {
        Self::Registry(error)
    }
}

impl From<TryReserveError> for Error {
    fn from(error: TryReserveError) -> Self {
        Self::Allocation(error)
    }
}
