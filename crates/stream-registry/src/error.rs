use std::{error::Error as StdError, fmt};

use sled::transaction::{ConflictableTransactionError, TransactionError};

use crate::{StreamId, StreamPartId};

#[derive(Debug)]
pub enum Error {
    Database(sled::Error),
    Serialization(postcard::Error),
    StreamNotFound(StreamId),
    StreamPartNotFound(StreamPartId),
    StreamAlreadyExists(StreamId),
    StreamPartAlreadyExists(StreamPartId),
    LengthOverflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => {
                write!(f, "stream registry database operation failed: {error}")
            }
            Self::Serialization(error) => {
                write!(f, "stream registry serialization failed: {error}")
            }
            Self::StreamNotFound(id) => write!(f, "stream {id} was not found"),
            Self::StreamPartNotFound(id) => write!(f, "stream part {id} was not found"),
            Self::StreamAlreadyExists(id) => write!(f, "stream {id} already exists"),
            Self::StreamPartAlreadyExists(id) => write!(f, "stream part {id} already exists"),
            Self::LengthOverflow => write!(f, "stream logical length exceeds u64"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Serialization(error) => Some(error),
            Self::StreamNotFound(_)
            | Self::StreamPartNotFound(_)
            | Self::StreamAlreadyExists(_)
            | Self::StreamPartAlreadyExists(_)
            | Self::LengthOverflow => None,
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
