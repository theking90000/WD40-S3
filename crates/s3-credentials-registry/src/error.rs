use std::{error::Error as StdError, fmt};

use crate::KeyId;

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
    InvalidKeyId,
    InvalidKeySecret,
    KeyAlreadyExists(KeyId),
    CorruptState(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "grpc-client")]
            Self::Transport(error) => write!(f, "registry transport failed: {error}"),
            #[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
            Self::Rpc(error) => write!(f, "registry RPC failed: {error}"),
            Self::Database(error) => write!(f, "credentials database operation failed: {error}"),
            Self::Serialization(error) => write!(f, "credentials serialization failed: {error}"),
            Self::FlushWorker(error) => {
                write!(f, "credentials flush worker could not start: {error}")
            }
            Self::FlushCanceled(error) => write!(f, "credentials flush worker stopped: {error}"),
            Self::InvalidKeyId => f.write_str("key ID must contain exactly 20 Unicode characters"),
            Self::InvalidKeySecret => {
                f.write_str("key secret must contain exactly 40 Unicode characters")
            }
            Self::KeyAlreadyExists(id) => write!(f, "key {:?} already exists", id.as_str()),
            Self::CorruptState(reason) => write!(f, "corrupt credentials registry state: {reason}"),
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

impl From<futures_channel::oneshot::Canceled> for Error {
    fn from(error: futures_channel::oneshot::Canceled) -> Self {
        Self::FlushCanceled(error)
    }
}
