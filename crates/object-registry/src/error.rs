use std::{error::Error as StdError, fmt};

use sled::transaction::{ConflictableTransactionError, TransactionError};

use crate::VersionId;

#[derive(Debug)]
pub enum Error {
    Database(sled::Error),
    Serialization(postcard::Error),
    FlushWorker(std::io::Error),
    FlushCanceled(futures_channel::oneshot::Canceled),
    InvalidKey,
    InvalidPrefix,
    InvalidLimit,
    InvalidCursor,
    ObjectNotFound(String),
    VersionNotFound { key: String, version_id: VersionId },
    CorruptState(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => {
                write!(f, "object registry database operation failed: {error}")
            }
            Self::Serialization(error) => {
                write!(f, "object registry serialization failed: {error}")
            }
            Self::FlushWorker(error) => {
                write!(f, "object registry flush worker could not start: {error}")
            }
            Self::FlushCanceled(error) => {
                write!(f, "object registry flush worker stopped: {error}")
            }
            Self::InvalidKey => write!(f, "object keys must be nonempty and contain no NUL"),
            Self::InvalidPrefix => write!(f, "object prefixes must contain no NUL"),
            Self::InvalidLimit => write!(f, "object listing limit must be greater than zero"),
            Self::InvalidCursor => write!(f, "invalid cursor for this object listing"),
            Self::ObjectNotFound(key) => write!(f, "object {key:?} was not found"),
            Self::VersionNotFound { key, version_id } => {
                write!(f, "version {version_id} of object {key:?} was not found")
            }
            Self::CorruptState(reason) => write!(f, "corrupt object registry state: {reason}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Serialization(error) => Some(error),
            Self::FlushWorker(error) => Some(error),
            Self::FlushCanceled(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sled::Error> for Error {
    fn from(error: sled::Error) -> Self {
        Self::Database(error)
    }
}

impl From<postcard::Error> for Error {
    fn from(error: postcard::Error) -> Self {
        Self::Serialization(error)
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(_: std::str::Utf8Error) -> Self {
        Self::CorruptState("object key is not UTF-8")
    }
}

impl From<futures_channel::oneshot::Canceled> for Error {
    fn from(error: futures_channel::oneshot::Canceled) -> Self {
        Self::FlushCanceled(error)
    }
}

impl From<TransactionError<Error>> for Error {
    fn from(error: TransactionError<Error>) -> Self {
        match error {
            TransactionError::Abort(error) => error,
            TransactionError::Storage(error) => Self::Database(error),
        }
    }
}

impl From<Error> for ConflictableTransactionError<Error> {
    fn from(error: Error) -> Self {
        Self::Abort(error)
    }
}
