use std::{error::Error as StdError, fmt};

/// Retained storage causes and blob adapter contract errors.
#[derive(Debug)]
pub enum Error {
    BlobRegistry(blob_registry::Error),
    Stream(stream_system::Error),
    LengthOverflow,
    MissingStreamId,
    InputClosed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlobRegistry(error) => write!(f, "blob registry operation failed: {error}"),
            Self::Stream(error) => write!(f, "blob stream operation failed: {error}"),
            Self::LengthOverflow => write!(f, "blob part length or slice end exceeds u64"),
            Self::MissingStreamId => write!(f, "closed blob part has no registered stream"),
            Self::InputClosed => write!(f, "blob part input is closed"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::BlobRegistry(error) => Some(error),
            Self::Stream(error) => Some(error),
            Self::LengthOverflow | Self::MissingStreamId | Self::InputClosed => None,
        }
    }
}

impl From<blob_registry::Error> for Error {
    fn from(error: blob_registry::Error) -> Self {
        Self::BlobRegistry(error)
    }
}

impl From<stream_system::Error> for Error {
    fn from(error: stream_system::Error) -> Self {
        Self::Stream(error)
    }
}

impl From<Error> for std::io::Error {
    fn from(error: Error) -> Self {
        Self::other(error)
    }
}
