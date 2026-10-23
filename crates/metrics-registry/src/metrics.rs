/// Metric identifiers shared by records and queries.
/// Persist the explicit numeric identifier; never renumber or reuse one.
/// Identifier 2, formerly Objects, remains reserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[repr(u16)]
pub enum Metric {
    StoredBytes = 1,
    DownloadedBytes = 4,
    UploadedBytes = 5,
    RequestCount = 3,
}
