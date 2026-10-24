//! Failures returned by metrics registry operations.

use std::{borrow::Cow, error::Error as StdError, fmt, time::Duration};

/// An invalid request or a failed metrics registry operation.
///
/// Validation errors reject the request before applying any mutations.
/// [`Error::Backend`] does not guarantee that a mutation was rolled back.
#[derive(Debug)]
pub enum Error {
    /// A connection to the remote registry failed.
    #[cfg(feature = "grpc-client")]
    Transport(tonic::transport::Error),
    /// A gRPC call or protocol conversion failed.
    #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
    Rpc(tonic::Status),
    /// Initialization policies are invalid, with a description of the reason.
    InvalidConfig(&'static str),
    /// A required column family was not opened by the caller.
    MissingColumnFamily(&'static str),
    /// A storage key or value has an invalid or unsupported binary representation.
    CorruptData(&'static str),
    /// A record violates the input contract, with a description of the reason.
    InvalidRecord(Cow<'static, str>),
    /// A query violates the input contract, with a description of the reason.
    InvalidQuery(Cow<'static, str>),
    /// The requested history resolution is not available in this registry.
    ResolutionUnavailable(Duration),
    /// Requested history is no longer retained at the selected resolution.
    HistoryExpired,
    /// A numeric result or time calculation cannot be represented, including an
    /// overflow discovered when evaluating a deferred native addition.
    Overflow,
    /// An implementation-specific operation failed; [`std::error::Error::source`]
    /// exposes the underlying error.
    Backend(Box<dyn StdError + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "grpc-client")]
            Self::Transport(error) => write!(formatter, "registry transport failed: {error}"),
            #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
            Self::Rpc(error) => write!(formatter, "registry RPC failed: {error}"),
            Self::InvalidConfig(reason) => {
                write!(formatter, "invalid metrics configuration: {reason}")
            }
            Self::MissingColumnFamily(name) => {
                write!(formatter, "missing metrics column family: {name}")
            }
            Self::CorruptData(reason) => {
                write!(formatter, "invalid metrics storage data: {reason}")
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
            #[cfg(feature = "grpc-client")]
            Self::Transport(error) => Some(error),
            #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
            Self::Rpc(error) => Some(error),
            Self::Backend(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
