use std::{error::Error as StdError, fmt, time::Duration};

/// Validation errors reject the request before applying any mutations.
/// Backend failures do not guarantee that a mutation was rolled back.
#[derive(Debug)]
pub enum Error {
    InvalidRecord(&'static str),
    InvalidQuery(&'static str),
    ResolutionUnavailable(Duration),
    HistoryExpired,
    Overflow,
    Backend(Box<dyn StdError + Send + Sync>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
