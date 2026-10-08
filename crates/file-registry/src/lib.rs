//! File registry library for WD40-S3.

pub mod local;
pub mod traits;

pub use backends::{FileBackend, RegistryBackend};
pub use local::LocalFileRegistry;
pub use traits::{FileAccess, FileId, FileRegistry, ReadAccessPath, WriteAccessPath};
