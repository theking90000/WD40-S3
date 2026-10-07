//! File registry library for WD40-S3.

pub mod traits;

pub use backends::{FileBackend, RegistryBackend};
pub use traits::{FileAccess, FileId, FileRegistry, ReadAccessPath, WriteAccessPath};
