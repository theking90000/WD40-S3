//! Stream I/O over file and stream registry enums.

mod async_read;
mod error;
mod read_file;
mod read_stream;
mod write_file;
mod write_stream;

pub use backends::FileBackend;
pub use carbon_io::{BytesFrameAllocator, FrameBudget, SchedulerConfig, Window};
pub use craft_codec::Compression;
pub use async_read::AsyncReadAdapter;
pub use error::Error;
pub use file_registry::FileRegistry;
pub use read_file::{ReadFile, ReadFrame};
pub use read_stream::ReadStream;
pub use stream_registry::{StreamId, StreamRegistry};
pub use write_file::{Encryption, StreamWriterConfig, WriteFile};
pub use write_stream::WriteStream;

/// Caller-configured registries and backend used to write streams.
pub struct StreamSystem {
    pub file_registry: FileRegistry,
    pub stream_registry: StreamRegistry,
    pub file_backend: FileBackend,
}
