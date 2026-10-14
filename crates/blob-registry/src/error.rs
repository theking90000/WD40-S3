use std::{error::Error as StdError, fmt};

use sled::transaction::{ConflictableTransactionError, TransactionError};

use crate::{BlobId, PartId};

#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "grpc-client")]
    Transport(tonic::transport::Error),
    #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
    Rpc(tonic::Status),
    Database(sled::Error),
    Serialization(postcard::Error),
    FlushWorker(std::io::Error),
    FlushCanceled(futures_channel::oneshot::Canceled),
    BlobNotFound(BlobId),
    BlobAlreadyExists(BlobId),
    BlobNotFinalized(BlobId),
    BlobFinalized(BlobId),
    PartNotFound {
        blob_id: BlobId,
        part_id: PartId,
    },
    InvalidRange {
        start: u64,
        end: u64,
        length: u64,
    },
    LengthOverflow,
    CorruptState(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "grpc-client")]
            Self::Transport(error) => write!(f, "registry transport failed: {error}"),
            #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
            Self::Rpc(error) => write!(f, "registry RPC failed: {error}"),
            Self::Database(error) => write!(f, "blob registry database operation failed: {error}"),
            Self::Serialization(error) => write!(f, "blob registry serialization failed: {error}"),
            Self::FlushWorker(error) => {
                write!(f, "blob registry flush worker could not start: {error}")
            }
            Self::FlushCanceled(error) => write!(f, "blob registry flush worker stopped: {error}"),
            Self::BlobNotFound(id) => write!(f, "blob {id} was not found"),
            Self::BlobAlreadyExists(id) => write!(f, "blob {id} already exists"),
            Self::BlobNotFinalized(id) => write!(f, "blob {id} is not finalized"),
            Self::BlobFinalized(id) => write!(f, "blob {id} is finalized"),
            Self::PartNotFound { blob_id, part_id } => {
                write!(f, "part {part_id} in blob {blob_id} was not found")
            }
            Self::InvalidRange { start, end, length } => {
                write!(
                    f,
                    "blob byte range {start}..{end} is invalid for length {length}"
                )
            }
            Self::LengthOverflow => write!(f, "blob logical length exceeds u64"),
            Self::CorruptState(reason) => write!(f, "corrupt blob registry state: {reason}"),
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
            Self::Database(error) => Some(error),
            Self::FlushWorker(error) => Some(error),
            Self::FlushCanceled(error) => Some(error),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sled::Error> for Error {
    fn from(error: sled::Error) -> Self {
        Self::Database(error)
    }
}

impl From<futures_channel::oneshot::Canceled> for Error {
    fn from(error: futures_channel::oneshot::Canceled) -> Self {
        Self::FlushCanceled(error)
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
