//! Stable identifiers for the metrics accepted by the registry API.

/// A metric identifier shared by records and queries.
///
/// Persist the explicit numeric identifier; never renumber or reuse one.
/// Identifier 2, formerly `Objects`, remains reserved. The numeric representation
/// does not itself define a serialization format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u16)]
pub enum Metric {
    /// Number of bytes currently stored.
    StoredBytes = 1,
    /// Number of downloaded bytes.
    DownloadedBytes = 2,
    /// Number of uploaded bytes.
    UploadedBytes = 3,
    /// Number of requests.
    RequestCount = 4,
}
