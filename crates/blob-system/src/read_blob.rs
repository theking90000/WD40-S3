use std::{
    future::{Future, poll_fn},
    ops::Range,
    pin::Pin,
    task::{Context, Poll, ready},
};

use blob_registry::traits::BlobRegistry as _;
use futures_core::Stream;
use futures_util::StreamExt;
use stream_system::ReadStream;

use crate::{
    AsyncReadAdapter, BlobId, BlobRegistry, BlobSystem, Error, FrameBudget, ReadFrame,
    SchedulerConfig,
};

type Slices = <BlobRegistry as blob_registry::traits::BlobRegistry>::StreamSliceStream;
type OpenSlices = Pin<Box<dyn Future<Output = Result<Slices, blob_registry::Error>>>>;

enum Resolution {
    Opening(OpenSlices),
    Reading(Slices),
    Finished,
}

/// A lazy blob reader backed by one internal ReadStream.
/// Reads slices in manifest order and swaps decoded frames without copying.
/// Only the shared budget and mutable CARBON configuration are borrowed.
/// Stream IDs and request admission remain internal.
pub struct ReadBlob<'a> {
    reader: Option<ReadStream<'a>>,
    resolution: Resolution,
    error: Option<Error>,
}

impl<'a> ReadBlob<'a> {
    /// Constructs CARBON and retains an unpolled blob-registry lookup future.
    /// Performs no metadata or backend I/O. Missing blobs, unfinalized blobs
    /// and invalid ranges are reported by polling. The end is exclusive.
    pub fn new(
        system: &BlobSystem,
        blob_id: BlobId,
        range: Range<u64>,
        budget: &'a FrameBudget,
        carbon_config: &'a mut SchedulerConfig,
    ) -> Result<Self, Error> {
        let reader = ReadStream::new(
            system.file_registry.clone(),
            system.stream_registry.clone(),
            system.backend.clone(),
            budget,
            carbon_config,
        )?;
        let registry = system.blob_registry.clone();
        Ok(Self {
            reader: Some(reader),
            resolution: Resolution::Opening(Box::pin(async move {
                registry.read(blob_id, range).await
            })),
            error: None,
        })
    }

    /// Exchanges one decoded frame. Only ReadFrame::selected belongs to the
    /// requested range. Pending, EOF and errors preserve the output frame.
    /// A fatal error cancels outstanding work, is reported once, then yields EOF.
    pub fn poll_read_frame(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut ReadFrame,
    ) -> Poll<Result<Option<()>, Error>> {
        if let Some(error) = self.error.take() {
            return Poll::Ready(Err(error));
        }
        // Bound synchronous transitions, including requests with no output.
        for _ in 0..32 {
            let plan_pending = match self.poll_requests(cx) {
                Poll::Ready(Ok(())) => false,
                Poll::Pending => true,
                Poll::Ready(Err(error)) => {
                    self.abort();
                    return Poll::Ready(Err(error));
                }
            };
            let Some(reader) = &mut self.reader else {
                return Poll::Ready(Ok(None));
            };
            match reader.poll_read_frame(cx, output) {
                Poll::Ready(Ok(Some(()))) => return Poll::Ready(Ok(Some(()))),
                Poll::Ready(Ok(None)) => {
                    self.abort();
                    return Poll::Ready(Ok(None));
                }
                Poll::Ready(Err(error)) => {
                    self.abort();
                    return Poll::Ready(Err(Error::Stream(error)));
                }
                Poll::Pending => {
                    if plan_pending || !reader.can_push_stream() {
                        return Poll::Pending;
                    }
                    // Admission may have changed during the frame poll.
                    // Install the next request here rather than await a wakeup
                    // from ReadStream's idle, open input.
                }
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }

    /// Awaitable frame exchange; cancellation preserves all partial operations.
    pub async fn read_frame(&mut self, output: &mut ReadFrame) -> Result<Option<()>, Error> {
        poll_fn(|cx| self.poll_read_frame(cx, output)).await
    }

    /// Advances metadata and file I/O without consuming frames.
    /// Errors are retained for the next frame read.
    pub fn poll_progress(&mut self, cx: &mut Context<'_>) {
        if let Poll::Ready(Err(error)) = self.poll_requests(cx) {
            self.abort();
            self.error = Some(error);
        }
    }

    /// Drives operations alongside an independent future. Never completes;
    /// use a frame read to observe errors or EOF.
    pub async fn progress(&mut self) {
        poll_fn(|cx| {
            self.poll_progress(cx);
            Poll::<()>::Pending
        })
        .await
    }

    /// Supplies futures-I/O byte reads by copying selected bytes to the caller.
    pub fn into_async_read(self) -> AsyncReadAdapter<'a> {
        AsyncReadAdapter::new(self)
    }

    // Pending means registry resolution is waiting. When the current request
    // needs consumption, available input work is done and returns Ready.
    fn poll_requests(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let Some(reader) = &mut self.reader else {
            return Poll::Ready(Ok(()));
        };
        if matches!(self.resolution, Resolution::Finished) {
            reader.poll_progress(cx);
            return Poll::Ready(Ok(()));
        }
        match reader.poll_can_push_stream(cx) {
            Poll::Pending => return Poll::Ready(Ok(())),
            Poll::Ready(result) => result?,
        }
        if let Resolution::Opening(future) = &mut self.resolution {
            let slices = ready!(future.as_mut().poll(cx))?;
            self.resolution = Resolution::Reading(slices);
        }
        let slice = match &mut self.resolution {
            Resolution::Reading(slices) => ready!(slices.poll_next_unpin(cx))
                .transpose()?,
            Resolution::Opening(_) | Resolution::Finished => None,
        };
        match slice {
            Some(slice) => {
                let end = slice
                    .skip
                    .checked_add(slice.length)
                    .ok_or(Error::LengthOverflow)?;
                reader
                    .push_stream(slice.stream_id, slice.skip, Some(end))?;
                reader.poll_progress(cx);
            }
            None => {
                self.resolution = Resolution::Finished;
                reader.close_stream();
                reader.poll_progress(cx);
            }
        }
        Poll::Ready(Ok(()))
    }

    fn abort(&mut self) {
        self.reader = None;
        self.resolution = Resolution::Finished;
    }
}

/// Convenient owned frames; use read_frame to recycle the previous buffer.
impl Stream for ReadBlob<'_> {
    type Item = Result<ReadFrame, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut frame = ReadFrame::default();
        match ready!(self.get_mut().poll_read_frame(cx, &mut frame)) {
            Ok(Some(())) => Poll::Ready(Some(Ok(frame))),
            Ok(None) => Poll::Ready(None),
            Err(error) => Poll::Ready(Some(Err(error))),
        }
    }
}
