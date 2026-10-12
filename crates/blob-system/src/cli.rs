//! Blob writers and readers reuse the stream codec and CARBON settings.

pub use stream_system::cli::{
    CompressionMode, EncryptionMode, StreamSchedulerConfig, StreamWriteConfig as BlobWriteConfig,
};
