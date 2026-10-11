use std::{error::Error as StdError, fmt};

use crate::{BlobId, PartId};

#[derive(Debug)]
pub enum Error {
    Database(sled::Error),
    Serialization(postcard::Error),
    BlobNotFound(BlobId),
    BlobAlreadyExists(BlobId),
    BlobNotFinalized(BlobId),
    BlobFinalized(BlobId),
    PartNotFound { blob_id: BlobId, part_id: PartId },
    PartAlreadyExists { blob_id: BlobId, part_id: PartId },
    InvalidRange { start: u64, end: u64, length: u64 },
    LengthOverflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "blob registry database operation failed: {error}"),
            Self::Serialization(error) => write!(f, "blob registry serialization failed: {error}"),
            Self::BlobNotFound(id) => write!(f, "blob {id} was not found"),
            Self::BlobAlreadyExists(id) => write!(f, "blob {id} already exists"),
            Self::BlobNotFinalized(id) => write!(f, "blob {id} is not finalized"),
            Self::BlobFinalized(id) => write!(f, "blob {id} is finalized"),
            Self::PartNotFound { blob_id, part_id } => {
                write!(f, "part {part_id} in blob {blob_id} was not found")
            }
            Self::PartAlreadyExists { blob_id, part_id } => {
                write!(f, "part {part_id} in blob {blob_id} already exists")
            }
            Self::InvalidRange { start, end, length } => {
                write!(
                    f,
                    "blob byte range {start}..{end} is invalid for length {length}"
                )
            }
            Self::LengthOverflow => write!(f, "blob logical length exceeds u64"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Database(error) => Some(error),
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

impl From<postcard::Error> for Error {
    fn from(error: postcard::Error) -> Self {
        Self::Serialization(error)
    }
}
