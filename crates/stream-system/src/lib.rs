//! Stream writing over file and stream registry enums.

mod error;
mod write_file;

pub use backends::FileBackend;
pub use craft_codec::Compression;
pub use error::Error;
pub use file_registry::FileRegistry;
pub use stream_registry::StreamRegistry;
pub use write_file::{Encryption, StreamWriterConfig, WriteFile};

/// Caller-configured registries and backend used to write streams.
pub struct StreamSystem {
    pub file_registry: FileRegistry,
    pub stream_registry: StreamRegistry,
    pub file_backend: FileBackend,
}
