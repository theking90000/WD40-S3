//! Failures returned by metrics registry operations.

use std::{error::Error as StdError, fmt, time::Duration};

/// An invalid request or a failed metrics registry operation.
///
/// Validation errors reject the request before applying any mutations.
/// [`Error::Backend`] does not guarantee that a mutation was rolled back.
#[derive(Debug)]
pub enum Error {
    /// The local storage backend is not implemented yet.
    LocalUnavailable,
    /// Initialization policies are invalid, with a description of the reason.
    InvalidConfig(&'static str),
    /// A required column family was not opened by the caller.
    MissingColumnFamily(&'static str),
    /// A record violates the input contract, with a description of the reason.
    InvalidRecord(&'static str),
    /// A query violates the input contract, with a description of the reason.
    InvalidQuery(&'static str),
    /// The requested history resolution is not available in this registry.
    ResolutionUnavailable(Duration),
    /// Requested history is no longer retained at the selected resolution.
    HistoryExpired,
    /// A numeric result or time calculation cannot be represented.
    Overflow,
    /// An implementation-specific operation failed; [`std::error::Error::source`]
    /// exposes the underlying error.
    Backend(Box<dyn StdError + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalUnavailable => {
                write!(formatter, "local metrics storage is not available yet")
            }
            Self::InvalidConfig(reason) => {
                write!(formatter, "invalid metrics configuration: {reason}")
            }
            Self::MissingColumnFamily(name) => {
                write!(formatter, "missing metrics column family: {name}")
            }
            Self::InvalidRecord(reason) => write!(formatter, "invalid metrics record: {reason}"),
            Self::InvalidQuery(reason) => write!(formatter, "invalid metrics query: {reason}"),
            Self::ResolutionUnavailable(resolution) => {
                write!(
                    formatter,
                    "metrics resolution {resolution:?} is unavailable"
                )
            }
            Self::HistoryExpired => write!(formatter, "requested metrics history has expired"),
            Self::Overflow => write!(formatter, "metrics arithmetic overflow"),
            Self::Backend(error) => write!(formatter, "metrics backend failed: {error}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Backend(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
