//! File registry library for WD40-S3.

pub mod traits;

pub use backends::{Backend, FileBackend};
pub use traits::{FileAccess, FileId, FileRegistry, ReadAccessPath, WriteAccessPath};
