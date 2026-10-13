//! Streams selected blob frames into S3S without copying their payloads.
//!
//! This module contains the transport compromise, so request handlers can remain
//! concerned with authorization, object lookup, ranges and response metadata.
//! The current producer task and channel are retained deliberately, but are not
//! the desired final design for CARBON's performance-sensitive read path.
//!
//! # Two separate constraints
//!
//! S3S 0.17 requires its `StreamingBlob` streams to be `Send + Sync + 'static`.
//! Its `DynByteStream` alias carries those bounds into the HTTP body. Our readers
//! contain futures and streams that guarantee `Send`, but not `Sync`. Polling a
//! reader requires exclusive mutable access; concurrent reader access is not a
//! requirement of CARBON or of the underlying `http_body::Body` trait.
//!
//! `SyncStream` satisfies S3S's type requirement by restricting access to the
//! inner stream to exclusive mutable polling. It adds no runtime lock, channel
//! or producer task. This compatibility wrapper must not be confused with the
//! separate channel used below to maintain reader progress.
//!
//! The second constraint is when the transport polls the body. A streaming body
//! is a pull interface: it runs when HTTP requests another frame. Hyper's HTTP/1
//! dispatcher can instead wait for a socket flush when its write buffer is full,
//! without calling the body's `poll_frame`. Backend wakeups can wake the HTTP
//! task, but that does not force Hyper to poll the body while writes are blocked.
//!
//! An async generator parked at `yield` therefore cannot keep calling the blob
//! reader's `poll_progress`. `read_frame` already drives CARBON while polled;
//! the missing work is progression during the interval when consumption stops.
//! Without that work, planned reads or decodes can wait until the next consumer
//! poll, reducing overlap between backend reads and downstream transmission.
//! Existing prefetch is not erased, and no percentage loss is established here.
//! This limitation also applies to the current generator in `http-adapter`.
//!
//! # Current compromise
//!
//! Each nonempty response starts one additional Tokio producer task. The HTTP
//! connection task already exists; no new thread or task is created per frame.
//! The producer owns the read resources and sends chunks through a capacity-one
//! channel. While a delivery is pending, `deliver` polls both CARBON progress
//! and the send future using the same producer-task context and waker.
//!
//! CARBON can continue its admitted work while HTTP is backpressured, up to its
//! existing window and shared frame budget. This does not authorize unlimited
//! reads or ignore downstream capacity. Once available work is exhausted, the
//! producer stays pending until an I/O, budget or channel wakeup occurs.
//!
//! The compromise adds channel synchronization, channel wakeups, runtime task
//! scheduling and another ownership handoff to the path. Those costs require
//! measurement; this is not a claim that the complete path remains lock-free,
//! nor that channel overhead has any particular measured magnitude. `Sync` alone
//! did not require us to introduce these costs.
//!
//! Channel capacity bounds queued chunks, not total retained frames or bytes.
//! In addition to one queued chunk, the producer may hold one chunk in a pending
//! send, and the initial response or HTTP may retain already-delivered chunks.
//! Decoded frames transferred out of CARBON are outside its shared frame budget.
//! Even a small selected slice retains its complete decoded frame allocation
//! until the final owning `Bytes` is dropped. Scheduler buffering is additional.
//!
//! The resources live in the producer task, so the response body owns no borrowed
//! scheduler or budget. Dropping the receiver wakes a pending send or the
//! explicit cancellation branch while reading. The producer then releases the
//! reader and its resources when scheduled; cancellation is not synchronous with
//! dropping the body. Cancellation of `read_blob` before its return also drops
//! the receiver. No join handle is needed to keep the task alive.
//!
//! # TODO: replace the task/channel bridge
//!
//! TODO(s3-body-progress): design one owning transport/response driver that polls
//! both network transmission and `ReadBlob::poll_progress` in the same task.
//! It must keep progressing admitted backend work when the socket is pending,
//! while consuming frames only when transmission has capacity. The target has
//! no producer task, channel or synchronized sharing of the mutable reader.
//!
//! TODO(s3-body-progress): establish the ownership and polling contract before
//! choosing a dependency fork. The mutable reader must remain exclusively owned;
//! two futures cannot independently borrow it while body consumption and
//! background progression are polled. A wrapper around the existing pull body
//! does not by itself create a transport-level progression hook.
//!
//! TODO(s3-body-progress): investigate an upstream S3S extension or a narrowly
//! scoped fork exposing the required response/transport integration. Removing
//! only `Sync` from `StreamingBlob` is insufficient: HTTP can still stop polling
//! the body. Hyper may also need an integration hook or a scoped driver change
//! to progress the reader while its dispatcher waits for the socket. Retain the
//! established S3 parsing, authentication, serialization and HTTP framing rather
//! than replacing the protocol stack to work around this ownership boundary.
//!
//! TODO(s3-body-progress): validate any replacement with a deliberately stalled
//! consumer. Observe backend progress reaching CARBON's permitted window without
//! draining it into an unbounded queue, then resuming correctly when HTTP resumes.
//! Check that full windows cause no busy polling, backend wakeups reach the
//! driver, and cancellation releases reads, frames and budget permits promptly.
//! Preserve ordering, exact range boundaries and errors before/after headers.
//!
//! TODO(s3-body-progress): compare throughput, allocations, CPU use, wakeups and
//! retained frame memory against this bridge and a direct CARBON reader. Include
//! fast/slow clients, fragmented reads, partial frames and concurrent responses.
//! Use measurements to justify the transport change and any maintained fork.

use std::{
    future::{Future, poll_fn},
    io,
    ops::Range,
};

use blob_system::{BlobId, ReadBlob, ReadFrame};
use bytes::Bytes;
use futures_util::StreamExt;
use s3s::{S3Error, S3Result, dto::StreamingBlob};
use sync_wrapper::SyncStream;
use tokio::sync::mpsc;

use crate::S3Adapter;

/// Reads the first frame before returning and drives CARBON during backpressure.
/// Nonempty bodies require a Tokio runtime. Dropping the body stops the reader.
///
/// Object authorization and range resolution have already happened in the
/// request handler. An empty selection creates no task and performs no blob I/O.
/// For a nonempty selection, the first delivered frame is prefetched so opening,
/// decoding and frame-validation failures can still become an S3 error before
/// response headers are sent. That frame is subsequently emitted exactly once.
/// Later failures are body-stream errors; they cannot replace committed headers
/// with an S3 XML error response.
pub(crate) async fn read_blob(
    adapter: &S3Adapter,
    blob_id: BlobId,
    selected: Range<u64>,
) -> S3Result<StreamingBlob> {
    let expected = selected
        .end
        .checked_sub(selected.start)
        .ok_or_else(|| s3s::s3_error!(InvalidRange))?;
    if expected == 0 {
        return Ok(StreamingBlob::from_bytes(Bytes::new()));
    }

    // The receiver owns the response side of the bridge. If this future is
    // cancelled during prefetch, dropping it also signals the producer to stop.
    let mut receiver = spawn_reader(adapter, blob_id, selected, expected)?;
    let first = receiver
        .recv()
        .await
        .ok_or_else(|| s3s::s3_error!(InternalError, "nonempty blob returned no frames"))?
        .map_err(S3Error::internal_error)?;
    // Reinsert the prefetched chunk before exposing the remaining channel items.
    // Polling this stream only receives; producer progression is independent of
    // these polls. SyncStream provides the S3S type bound, not that progression.
    let stream = futures_util::stream::poll_fn(move |cx| receiver.poll_recv(cx));
    let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(first) }).chain(stream);
    Ok(StreamingBlob::wrap(SyncStream::new(stream)))
}

/// Owns the resources borrowed by ReadBlob and starts the temporary producer.
/// The runtime lookup is fallible so library use outside Tokio returns an error.
fn spawn_reader(
    adapter: &S3Adapter,
    blob_id: BlobId,
    selected: Range<u64>,
    expected: u64,
) -> S3Result<mpsc::Receiver<io::Result<Bytes>>> {
    let runtime = tokio::runtime::Handle::try_current().map_err(S3Error::internal_error)?;
    let blobs = adapter.blobs.clone();
    let budget = adapter.budget.clone();
    let mut scheduler = adapter.config.scheduler;
    // Keep the bridge small. Increasing this capacity would retain more frames
    // outside CARBON's budget and would not solve the transport ownership issue.
    let (sender, receiver) = mpsc::channel(1);

    runtime.spawn(async move {
        let result: io::Result<()> = async {
            let mut reader = blobs
                .read(blob_id, selected, &budget, &mut scheduler)
                .map_err(io::Error::from)?;
            let mut remaining = expected;
            let mut frame = ReadFrame::default();
            loop {
                // A frame-read poll already drives CARBON. The second branch
                // here is cancellation, not another mutable reader operation.
                // Prioritize closure so a disconnected client stops new reads.
                let next = tokio::select! {
                    biased;
                    _ = sender.closed() => return Ok(()),
                    result = reader.read_frame(&mut frame) => result.map_err(io::Error::from)?,
                };
                if next.is_none() {
                    break;
                }
                let bytes: Bytes = SelectedFrame::try_from(std::mem::take(&mut frame))?.into();
                // Reject an oversized chunk before transferring it to HTTP.
                // Cancellation can stop the loop before the final EOF check;
                // a cancelled response is not treated as a completed transfer.
                account_bytes(&mut remaining, bytes.len())?;
                if !deliver(&mut reader, &sender, bytes).await {
                    return Ok(());
                }
            }
            if remaining != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "blob is shorter than recorded size",
                ));
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            // Preserve already-queued data order, then deliver the terminal
            // error. A closed receiver rejects it immediately. The reader has
            // already been dropped, so this wait performs no more backend work.
            let _ = sender.send(Err(error)).await;
        }
    });
    Ok(receiver)
}

/// Polls CARBON alongside a pending delivery instead of parking the reader.
///
/// Both futures share one task context. Backend and channel events wake that
/// task, whose next poll advances the reader before retrying the send. The
/// pending send future is retained across polls and pinned on the stack.
/// This is event-driven; there is no timer, spin loop or repeated task creation.
/// A closed receiver resolves the send with an error and terminates production.
///
/// TODO(s3-body-progress): keep this guarantee in the future single-task driver.
/// Replacing this function with send().await alone would again park CARBON at
/// the downstream capacity boundary, defeating the purpose of this bridge.
async fn deliver(
    reader: &mut ReadBlob<'_>,
    sender: &mpsc::Sender<io::Result<Bytes>>,
    bytes: Bytes,
) -> bool {
    let mut sending = std::pin::pin!(sender.send(Ok(bytes)));
    poll_fn(|cx| {
        reader.poll_progress(cx);
        sending.as_mut().poll(cx)
    })
    .await
    .is_ok()
}

/// Accounts for selected plaintext bytes, with checked conversion/subtraction.
/// Counts describe the requested range, not complete decoded-frame allocations.
fn account_bytes(remaining: &mut u64, count: usize) -> io::Result<()> {
    let count = u64::try_from(count).map_err(io::Error::other)?;
    *remaining = remaining
        .checked_sub(count)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "blob exceeds recorded size"))?;
    Ok(())
}

/// Owns the complete decoded frame while exposing only its validated selection.
///
/// Moving this owner into Bytes avoids copying the decoded payload. It does not
/// mean zero allocation: Bytes::from_owner allocates ownership bookkeeping, and
/// the reader still needs decoded buffers. A buffer cannot be recycled back into
/// CARBON while HTTP or any cloned Bytes retains its ownership.
struct SelectedFrame(ReadFrame);

impl TryFrom<ReadFrame> for SelectedFrame {
    type Error = io::Error;

    fn try_from(frame: ReadFrame) -> io::Result<Self> {
        if frame.selected.is_empty() || frame.buffer.get(frame.selected.clone()).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid decoded frame selection",
            ));
        }
        Ok(Self(frame))
    }
}

impl AsRef<[u8]> for SelectedFrame {
    fn as_ref(&self) -> &[u8] {
        // Construction validated the bounds and the private owner never mutates
        // them. Keep the access checked anyway, without a panicking index or an
        // unchecked slice. The empty fallback is not the normal streaming path.
        match self.0.buffer.get(self.0.selected.clone()) {
            Some(bytes) => bytes,
            None => &[],
        }
    }
}

impl From<SelectedFrame> for Bytes {
    fn from(frame: SelectedFrame) -> Self {
        Self::from_owner(frame)
    }
}
