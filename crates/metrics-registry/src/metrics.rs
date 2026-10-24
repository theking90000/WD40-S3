//! Built-in metric identifiers. Custom identifiers can also implement `Into<u16>`.

/// A metric identifier shared by records and queries.
///
/// Persist the explicit numeric identifier; never renumber or reuse one.
/// The local backend encodes this identifier as a big-endian `u16`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u16)]
pub enum Metric {
    /// Number of bytes currently stored, updated through signed gauge deltas.
    StoredBytes = 1,
    /// Number of downloaded bytes.
    DownloadedBytes = 2,
    /// Number of uploaded bytes.
    UploadedBytes = 3,
    /// Number of requests.
    RequestCount = 4,
    /// Number of explicitly successful requests.
    SuccessRequestCount = 5,
    /// Number of failed or abandoned requests.
    FailedRequestCount = 6,
    /// Number of objects currently stored.
    StoredObjects = 7,
}

impl From<Metric> for u16 {
    fn from(metric: Metric) -> Self {
        metric as u16
    }
}
