use std::{convert::Infallible, error::Error as StdError, fmt, io};

/// Failures while constructing a stream or opening, reading, writing, and finalizing its files.
#[derive(Debug)]
pub enum Error {
    Random(getrandom::Error),
    Codec(craft_codec::Error),
    FileRegistry(file_registry::Error),
    StreamRegistry(stream_registry::Error),
    Backend(backends::Error),
    Carbon(carbon_io::ContractError),
    Io(io::Error),
    FileNotOpen,
    FrameWritePending,
    FrameCapacityExceeded,
    FileTooSmall { limit: u64 },
    ReadStreamPending(stream_registry::StreamId),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random(error) => write!(f, "stream key generation failed: {error}"),
            Self::Codec(error) => write!(f, "stream codec failed: {error}"),
            Self::FileRegistry(error) => write!(f, "stream file registry operation failed: {error}"),
            Self::StreamRegistry(error) => write!(f, "stream registry operation failed: {error}"),
            Self::Backend(error) => write!(f, "stream backend operation failed: {error}"),
            Self::Carbon(error) => write!(f, "CARBON scheduler contract failed: {error}"),
            Self::Io(error) => write!(f, "stream file I/O failed: {error}"),
            Self::FileNotOpen => f.write_str("stream file is not open"),
            Self::FrameWritePending => f.write_str("cannot close a stream file with a pending frame write"),
            Self::FrameCapacityExceeded => f.write_str("stream file frame capacity exceeded"),
            Self::FileTooSmall { limit } => {
                write!(f, "maximum file size of {limit} bytes cannot fit one configured frame")
            }
            Self::ReadStreamPending(id) => write!(f, "stream {id} has not been fully consumed"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Random(error) => Some(error),
            Self::Codec(error) => Some(error),
            Self::FileRegistry(error) => Some(error),
            Self::StreamRegistry(error) => Some(error),
            Self::Backend(error) => Some(error),
            Self::Carbon(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::FileNotOpen | Self::FrameWritePending | Self::FrameCapacityExceeded | Self::FileTooSmall { .. }
                | Self::ReadStreamPending(_) => None,
        }
    }
}

impl From<carbon_io::SchedulerError<Error>> for Error {
    fn from(error: carbon_io::SchedulerError<Error>) -> Self {
        match error {
            carbon_io::SchedulerError::Backend(error) => error,
            carbon_io::SchedulerError::Contract(error) => Self::Carbon(error),
        }
    }
}

impl From<carbon_io::WriteError<Infallible, Error>> for Error {
    fn from(error: carbon_io::WriteError<Infallible, Error>) -> Self {
        match error {
            carbon_io::WriteError::Input(error) => match error {},
            carbon_io::WriteError::Backend(error) => error,
            carbon_io::WriteError::Contract(error) => Self::Carbon(error),
        }
    }
}
