use std::{error::Error as StdError, fmt, io};

/// Failures while opening, writing, or finalizing a stream file.
#[derive(Debug)]
pub enum Error {
    Random(getrandom::Error),
    Codec(craft_codec::Error),
    FileRegistry(file_registry::Error),
    Backend(backends::Error),
    Io(io::Error),
    FileNotOpen,
    FrameWritePending,
    FrameCapacityExceeded,
    FileTooSmall { limit: u64 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random(error) => write!(f, "stream key generation failed: {error}"),
            Self::Codec(error) => write!(f, "stream codec failed: {error}"),
            Self::FileRegistry(error) => write!(f, "stream file registry operation failed: {error}"),
            Self::Backend(error) => write!(f, "stream backend operation failed: {error}"),
            Self::Io(error) => write!(f, "stream file writing failed: {error}"),
            Self::FileNotOpen => f.write_str("stream file is not open"),
            Self::FrameWritePending => f.write_str("cannot close a stream file with a pending frame write"),
            Self::FrameCapacityExceeded => f.write_str("stream file frame capacity exceeded"),
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
            Self::Io(error) => Some(error),
            Self::FileNotOpen | Self::FrameWritePending | Self::FrameCapacityExceeded | Self::FileTooSmall { .. } => None,
        }
    }
}
