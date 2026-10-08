//! File registry library for WD40-S3.

pub mod error;
pub mod local;
pub mod traits;

pub use backends::{FileBackend, RegistryBackend};
pub use error::Error;
pub use local::{LocalFile, LocalRegistry};
pub use traits::{FileAccess, FileId, FileRegistry, ReadAccessPath, WriteAccessPath};
