use std::{error::Error as StdError, fmt};

use sled::transaction::{ConflictableTransactionError, TransactionError};

use crate::BlobId;

#[derive(Debug)]
pub enum Error {
    #[cfg(feature = "grpc-client")]
    Transport(tonic::transport::Error),
    #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
    Rpc(tonic::Status),
    Database(sled::Error),
    Serialization(postcard::Error),
    Random(getrandom::Error),
    FlushWorker(std::io::Error),
    FlushCanceled(futures_channel::oneshot::Canceled),
    InvalidUploadId,
    InvalidBucket,
    InvalidPrefix,
    InvalidCursor,
    InvalidLimit,
    BlobAlreadyExists(BlobId),
    CorruptState(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "grpc-client")]
            Self::Transport(error) => write!(f, "registry transport failed: {error}"),
            #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
            Self::Rpc(error) => write!(f, "registry RPC failed: {error}"),
            Self::Database(error) => write!(f, "multipart database operation failed: {error}"),
            Self::Serialization(error) => write!(f, "multipart serialization failed: {error}"),
            Self::Random(error) => write!(f, "multipart ID generation failed: {error}"),
            Self::FlushWorker(error) => {
                write!(f, "multipart flush worker could not start: {error}")
            }
            Self::FlushCanceled(error) => write!(f, "multipart flush worker stopped: {error}"),
            Self::InvalidUploadId => {
                f.write_str("upload ID must contain exactly 32 lowercase hexadecimal characters")
            }
            Self::InvalidBucket => f.write_str("invalid listing bucket"),
            Self::InvalidPrefix => f.write_str("invalid listing prefix"),
            Self::InvalidCursor => f.write_str("invalid listing cursor"),
            Self::InvalidLimit => f.write_str("listing limit must be between 1 and 1,000"),
            Self::BlobAlreadyExists(id) => write!(f, "blob {id} already has a multipart upload"),
            Self::CorruptState(reason) => write!(f, "corrupt multipart registry state: {reason}"),
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
            Self::Serialization(error) => Some(error),
            Self::Random(error) => Some(error),
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

impl From<getrandom::Error> for Error {
    fn from(error: getrandom::Error) -> Self {
        Self::Random(error)
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
