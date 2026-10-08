use std::{error::Error as StdError, fmt};

use crate::FileId;

#[derive(Debug)]
pub enum Error {
    Database(sled::Error),
    Serialization(postcard::Error),
    FileNotFound(FileId),
    FilePending(FileId),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "registry database operation failed: {error}"),
            Self::Serialization(error) => write!(f, "registry serialization failed: {error}"),
            Self::FileNotFound(id) => write!(f, "file {id} was not found"),
            Self::FilePending(id) => write!(f, "file {id} is still pending"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::Serialization(_) | Self::FileNotFound(_) | Self::FilePending(_) => None,
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
