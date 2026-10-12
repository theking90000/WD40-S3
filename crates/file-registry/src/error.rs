use std::{error::Error as StdError, fmt};

use sled::transaction::TransactionError;

use crate::FileId;

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
    Backend(backends::Error),
    FileNotFound(FileId),
    FilePending(FileId),
    MissingReadAccessPath(FileId),
    MissingWriteAccessPath,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "grpc-client")]
            Self::Transport(error) => write!(f, "registry transport failed: {error}"),
            #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
            Self::Rpc(error) => write!(f, "registry RPC failed: {error}"),
            Self::Database(error) => write!(f, "registry database operation failed: {error}"),
            Self::Serialization(error) => write!(f, "registry serialization failed: {error}"),
            Self::FlushWorker(error) => {
                write!(f, "file registry flush worker could not start: {error}")
            }
            Self::FlushCanceled(error) => write!(f, "file registry flush worker stopped: {error}"),
            Self::Backend(error) => write!(f, "registry backend operation failed: {error}"),
            Self::FileNotFound(id) => write!(f, "file {id} was not found"),
            Self::FilePending(id) => write!(f, "file {id} is still pending"),
            Self::MissingReadAccessPath(id) => {
                write!(f, "backend returned no read access path for file {id}")
            }
            Self::MissingWriteAccessPath => {
                f.write_str("backend returned too few write access paths")
            }
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
            Self::Backend(error) => Some(error),
            Self::Serialization(_)
            | Self::FileNotFound(_)
            | Self::FilePending(_)
            | Self::MissingWriteAccessPath
            | Self::MissingReadAccessPath(_) => None,
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

impl From<backends::Error> for Error {
    fn from(error: backends::Error) -> Self {
        Self::Backend(error)
    }
}

impl From<TransactionError<sled::Error>> for Error {
    fn from(error: TransactionError<sled::Error>) -> Self {
        match error {
            TransactionError::Abort(error) | TransactionError::Storage(error) => {
                Self::Database(error)
            }
        }
    }
}
