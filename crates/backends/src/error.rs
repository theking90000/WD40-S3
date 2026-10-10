use std::{error::Error as StdError, fmt};

use tokio_tcp_pool::AcquireError;

#[derive(Debug)]
pub enum Error {
    MissingBotCredentials,
    Acquire(AcquireError),
    Discord(discord_webhook_storage::Error),
    BackendMismatch,
    UnsupportedBackend,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBotCredentials => f.write_str("no bot credentials available for renewal"),
            Self::Acquire(error) => write!(f, "connection acquisition failed: {error}"),
            Self::Discord(error) => write!(f, "Discord request failed: {error}"),
            Self::BackendMismatch => f.write_str("access path belongs to another backend"),
            Self::UnsupportedBackend => f.write_str("backend operation is not implemented"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Acquire(error) => Some(error),
            Self::Discord(error) => Some(error),
            Self::MissingBotCredentials | Self::BackendMismatch | Self::UnsupportedBackend => None,
        }
    }
}

impl From<AcquireError> for Error {
    fn from(error: AcquireError) -> Self {
        Self::Acquire(error)
    }
}

impl From<discord_webhook_storage::Error> for Error {
    fn from(error: discord_webhook_storage::Error) -> Self {
        Self::Discord(error)
    }
}
