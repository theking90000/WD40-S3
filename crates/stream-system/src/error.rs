use std::{error::Error as StdError, fmt};

/// Failures while opening a stream file.
#[derive(Debug)]
pub enum Error {
    Random(getrandom::Error),
    Codec(craft_codec::Error),
    FileRegistry(file_registry::Error),
    Backend(backends::Error),
    FileTooSmall { limit: u64 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random(error) => write!(f, "stream key generation failed: {error}"),
            Self::Codec(error) => write!(f, "stream codec initialization failed: {error}"),
            Self::FileRegistry(error) => write!(f, "stream file allocation failed: {error}"),
            Self::Backend(error) => write!(f, "stream backend opening failed: {error}"),
            Self::FileTooSmall { limit } => {
                write!(f, "maximum file size of {limit} bytes cannot fit one configured frame")
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Random(error) => Some(error),
            Self::Codec(error) => Some(error),
            Self::FileRegistry(error) => Some(error),
            Self::Backend(error) => Some(error),
            Self::FileTooSmall { .. } => None,
        }
    }
}
