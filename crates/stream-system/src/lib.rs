//! Store byte streams and read their logical ranges across registered files.
//!
//! [`WriteStream`] implements [`futures_io::AsyncWrite`]. Successful closing
//! finalizes the files, registers their CRAFT metadata and stores an ordered
//! manifest. [`WriteStream::stream_id`] then returns its [`StreamId`]. Flushing
//! is a no-op and does not finalize the stream.
//!
//! [`ReadStream`] accepts one [`StreamId`] and logical `[start, end)` selection
//! through [`ReadStream::push_stream`]. It resolves parts progressively and
//! returns [`ReadFrame`] buffers in order. Another request is accepted after
//! [`ReadStream::can_push_stream`] becomes true. [`ReadStream::close_stream`]
//! seals request input; normal EOF follows consumption of the final request.
//! [`AsyncReadAdapter`] provides optional [`futures_io::AsyncRead`] access by
//! copying the selected bytes into the caller's destination.
//!
//! # Resources and progression
//!
//! Constructors take owned registry and backend handles. These enum handles
//! can be cloned to retain access in the caller. Writers borrow a
//! [`BytesFrameAllocator`], a shared [`FrameBudget`] and a mutable
//! [`SchedulerConfig`]; readers borrow the budget and configuration. The
//! configuration cannot be reused until its reader or writer is dropped.
//!
//! No internal task drives a stream. Polling reads, writes, closing or a
//! `progress()` future advances its operations. Registry lookup, opening and
//! finalization futures remain stored between polls. Dropping a stream cancels
//! its local operations; it does not roll back already registered objects.
//!
//! # Polling and request input
//!
//! Pushing or sealing read input does not wake a task. Poll the reader again
//! after either mutation. Between requests, open input returns `Pending`, not
//! EOF. Read progression does not consume frames and cannot make a request
//! available again while selected frames remain undelivered.
//!
//! In a guarded `select!`, compute request admission before creating the
//! futures and enable the request source only when admission is available.
//! The guard is fixed for that invocation. A frame-driving application helper
//! must also return when an empty request finishes, so the loop can recompute
//! the guard. [`ReadStream::poll_can_push_stream`] exposes admission readiness;
//! [`ReadStream::poll_read_frame`] drives and consumes frames. Both require
//! exclusive access to the same reader. A combined helper can poll frame
//! output and check the admission predicate in the same closure.
//!
//! # File adapters and features
//!
//! [`ReadFile`] and [`WriteFile`] implement the corresponding CARBON traits for
//! applications that drive individual files. A read file owns its metadata and
//! cursor. A write file finalizes to a stream part ID, not a complete stream ID.
//!
//! The default feature set does not enable the CLI or Clap. The library does
//! not start a runtime; backend-specific runtime requirements still apply. The
//! optional `cli` feature enables the upload binary. Byte-oriented extension
//! methods such as `write_all`, `close` and `read` can be supplied by
//! `futures-util` with its `io` feature. The I/O traits are from `futures-io`,
//! rather than Tokio's separate I/O traits.

/// Command-line codec and CARBON settings.
#[cfg(feature = "cli")]
pub mod cli;

/// Byte-oriented adaptation of decoded frames.
mod async_read;
/// Failures from registries, codecs, backends and stream lifecycle contracts.
mod error;
/// Owned metadata cursors and CARBON read-file integration.
mod read_file;
/// Pushed stream requests and progressive part resolution.
mod read_stream;
/// Codec settings and CARBON write-file integration.
mod write_file;
/// Byte ingestion, file scheduling and manifest finalization.
mod write_stream;

pub use async_read::AsyncReadAdapter;
/// Runtime-selected backend handle passed to readers and writers.
pub use backends::FileBackend;
/// Caller-owned CARBON allocation, shared frame-budget and scheduling resources.
pub use carbon_io::{BytesFrameAllocator, FrameBudget, SchedulerConfig, Window};
/// Compression profile applied independently to each encoded frame.
pub use craft_codec::Compression;
pub use error::Error;
/// Runtime-selected registry for file IDs and backend access paths.
pub use file_registry::FileRegistry;
pub use read_file::{ReadFile, ReadFrame};
pub use read_stream::ReadStream;
/// Stream identifiers and the runtime-selected manifest/metadata registry.
pub use stream_registry::{StreamId, StreamRegistry};
pub use write_file::{Encryption, StreamWriterConfig, WriteFile};
pub use write_stream::WriteStream;

/// Registry and backend handles shared by stream readers and writers.
///
/// This is a resource bundle. Stream constructors are on [`WriteStream`] and
/// [`ReadStream`]; constructing this bundle performs no I/O. The CARBON budget,
/// configuration and write allocator are supplied separately by the caller.
pub struct StreamSystem {
    /// Registry that allocates file IDs and resolves their backend access paths.
    pub file_registry: FileRegistry,
    /// Registry that stores ordered manifests and per-file CRAFT metadata.
    pub stream_registry: StreamRegistry,
    /// Backend used to open file readers and writers.
    pub file_backend: FileBackend,
}
