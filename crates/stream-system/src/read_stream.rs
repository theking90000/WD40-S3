use std::{
    future::{Future, poll_fn},
    ops::Range,
    pin::Pin,
    task::{Context, Poll},
};

use carbon_io::{FrameBudget, PushError, ReadScheduler, SchedulerConfig};
use futures_util::StreamExt;
use stream_registry::traits::StreamRegistry as _;

use crate::{Error, FileBackend, FileRegistry, ReadFile, ReadFrame, StreamId, StreamRegistry};

type Parts = <StreamRegistry as stream_registry::traits::StreamRegistry>::StreamPartStream;
type OpenParts = Pin<Box<dyn Future<Output = Result<Parts, Error>>>>;

enum Resolution {
    Opening(OpenParts),
    ReadingParts(Parts),
    Finished,
}

struct CurrentStream {
    stream_id: StreamId,
    start: u64,
    end: Option<u64>,
    resolution: Resolution,
    position_in_stream: u64,
    frames_remaining: u64,
    next_file: Option<ReadFile>,
}

impl CurrentStream {
    fn select_part(&mut self, length: u64) -> Result<Option<Range<u64>>, Error> {
        let position = self.position_in_stream;
        let next = position.checked_add(length)
            .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
        let start = self.start.max(position);
        let end = self.end.unwrap_or(next).min(next);
        let selection = if start < end {
            let local_start = start.checked_sub(position)
                .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
            let local_end = end.checked_sub(position)
                .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
            Some(local_start..local_end)
        } else {
            None
        };
        self.position_in_stream = next;
        if self.end.is_some_and(|end| next >= end) {
            self.resolution = Resolution::Finished;
        }
        Ok(selection)
    }

    fn finish_resolution(&mut self) -> Result<(), Error> {
        if self.start > self.position_in_stream
            || self.end.is_some_and(|end| end > self.position_in_stream)
        {
            return Err(Error::Codec(craft_codec::Error::InvalidRange));
        }
        self.resolution = Resolution::Finished;
        Ok(())
    }

    fn files_admitted(&self) -> bool {
        matches!(self.resolution, Resolution::Finished) && self.next_file.is_none()
    }

    fn consumed(&self) -> bool {
        self.files_admitted() && self.frames_remaining == 0
    }
}

/// Reads pushed stream selections in order, resolving their parts incrementally.
///
/// Only one request is current. Supply another when can_push_stream() becomes
/// true, after all selected frames have been delivered. Call close_stream()
/// to seal input; EOF follows consumption of the current request.
///
/// Each file owns its metadata. CARBON bounds active files and frame buffers;
/// the current request may hold one file refused by its admission window.
/// Registry futures, part discovery and backend I/O are polled directly without
/// tasks. Even when discovery is pending, admitted files continue progressing.
pub struct ReadStream<'a> {
    file_registry: FileRegistry,
    stream_registry: StreamRegistry,
    backend: FileBackend,
    carbon: Option<ReadScheduler<'a, ReadFrame, ReadFile, fn() -> ReadFrame>>,
    current: Option<CurrentStream>,
    input_closed: bool,
    error: Option<Error>,
}

impl<'a> ReadStream<'a> {
    /// Borrows only the caller's shared frame budget and CARBON configuration.
    pub fn new(
        file_registry: FileRegistry,
        stream_registry: StreamRegistry,
        backend: FileBackend,
        budget: &'a FrameBudget,
        carbon_config: &'a mut SchedulerConfig,
    ) -> Result<Self, Error> {
        let carbon = ReadScheduler::new(
            ReadFrame::default as fn() -> ReadFrame, budget, carbon_config,
        ).map_err(Error::Carbon)?;
        Ok(Self {
            file_registry,
            stream_registry,
            backend,
            carbon: Some(carbon),
            current: None,
            input_closed: false,
            error: None,
        })
    }

    /// Installs the current stream and its half-open logical selection.
    /// None reads through its logical EOF. Registry lookup starts on polling.
    /// Bounds beyond EOF are detected during resolution, possibly after output.
    /// Explicitly poll after pushing; this mutation does not wake a task.
    pub fn push_stream(&mut self, stream_id: StreamId, start: u64, end: Option<u64>) -> Result<(), Error> {
        if self.input_closed {
            return Err(Error::Carbon(carbon_io::ContractError::InputClosed));
        }
        if let Some(current) = &self.current {
            return Err(Error::ReadStreamPending(current.stream_id));
        }
        if end.is_some_and(|end| end < start) {
            return Err(Error::Codec(craft_codec::Error::InvalidRange));
        }
        let registry = self.stream_registry.clone();
        self.current = Some(CurrentStream {
            stream_id,
            start,
            end,
            resolution: Resolution::Opening(Box::pin(async move {
                registry.retrieve_parts(&[stream_id]).await.map_err(Error::StreamRegistry)
            })),
            position_in_stream: 0,
            frames_remaining: 0,
            next_file: None,
        });
        Ok(())
    }

    /// True after resolution finishes and every selected frame is delivered.
    pub fn can_push_stream(&self) -> bool {
        !self.input_closed && self.current.is_none()
    }

    /// Drives discovery and I/O while checking admission of another stream.
    /// Does not consume frames. This also exposes completion of empty selections.
    /// Backend errors remain available through poll_read_frame().
    pub fn poll_can_push_stream(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        self.poll_progress(cx);
        if let Some(error) = self.error.take() {
            return Poll::Ready(Err(error));
        }
        if self.input_closed {
            return Poll::Ready(Err(Error::Carbon(carbon_io::ContractError::InputClosed)));
        }
        if self.can_push_stream() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    /// Seals stream input, allowing the current request to finish normally.
    /// CARBON input closes once its remaining files have been admitted.
    /// Explicitly poll again after closing; this mutation does not wake a task.
    pub fn close_stream(&mut self) {
        self.input_closed = true;
        self.finish_current();
    }

    /// Drives resolution and CARBON, exchanging the next frame without copying.
    /// Pending, EOF and errors preserve the caller's frame. EOF requires closed
    /// input; between requests, open input waits for another push_stream().
    /// A fatal error cancels ongoing operations and releases the frame budget.
    pub fn poll_read_frame(
        &mut self,
        cx: &mut Context<'_>,
        output: &mut ReadFrame,
    ) -> Poll<Result<Option<()>, Error>> {
        if let Some(error) = self.error.take() {
            return Poll::Ready(Err(error));
        }
        let resolution = self.resolve_current(cx);
        self.finish_current();
        if let Err(error) = resolution {
            if let Some(carbon) = &mut self.carbon {
                carbon.poll_progress(cx);
            }
            self.abort();
            return Poll::Ready(Err(error));
        }
        let Some(carbon) = &mut self.carbon else {
            return Poll::Ready(Ok(None));
        };
        match carbon.poll_read(cx, output) {
            Poll::Ready(Ok(Some(()))) => {
                let remaining = self.current.as_ref()
                    .and_then(|current| current.frames_remaining.checked_sub(1));
                if let (Some(current), Some(remaining)) = (&mut self.current, remaining) {
                    current.frames_remaining = remaining;
                    self.finish_current();
                } else {
                    // A swapped frame is acknowledged before reporting a later error.
                    self.fail(Error::Carbon(carbon_io::ContractError::InvalidFramePosition));
                }
                Poll::Ready(Ok(Some(())))
            }
            Poll::Ready(Err(error)) => {
                self.abort();
                Poll::Ready(Err(Error::from(error)))
            }
            result => result.map(|result| result.map_err(Error::from)),
        }
    }

    /// Cancellation preserves resolution, admitted files and partial frame reads.
    pub async fn read_frame(&mut self, output: &mut ReadFrame) -> Result<Option<()>, Error> {
        poll_fn(|cx| self.poll_read_frame(cx, output)).await
    }

    /// Advances resolution and backend I/O without consuming an output frame.
    /// Errors are retained for the next frame read. Backend waits use the caller's
    /// waker directly; empty open input requires a push followed by another poll.
    pub fn poll_progress(&mut self, cx: &mut Context<'_>) {
        if self.carbon.is_none() {
            return;
        }
        let resolution = self.resolve_current(cx);
        self.finish_current();
        if let Some(carbon) = &mut self.carbon {
            carbon.poll_progress(cx);
        }
        if let Err(error) = resolution {
            self.fail(error);
        }
    }

    /// Drives without consuming frames. Never completes; errors appear on read.
    pub async fn progress(&mut self) {
        poll_fn(|cx| {
            self.poll_progress(cx);
            Poll::<()>::Pending
        }).await
    }

    fn resolve_current(&mut self, cx: &mut Context<'_>) -> Result<(), Error> {
        let (Some(current), Some(carbon)) = (&mut self.current, &mut self.carbon) else {
            return Ok(());
        };
        // Bound synchronous discovery work so existing files are polled every turn.
        for _ in 0..32 {
            if let Some(file) = current.next_file.take() {
                let count = u64::from(carbon_io::ReadFile::frame_count(&file));
                let remaining = current.frames_remaining.checked_add(count)
                    .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
                match carbon.push_file(file) {
                    Ok(()) => current.frames_remaining = remaining,
                    Err((PushError::Full | PushError::Closed, file)) => {
                        // A terminal CARBON error is still reported by its frame read.
                        current.next_file = Some(file);
                        return Ok(());
                    }
                    Err((PushError::Contract(error), _)) => return Err(Error::Carbon(error)),
                }
            }
            match &mut current.resolution {
                Resolution::Opening(future) => match future.as_mut().poll(cx) {
                    Poll::Pending => return Ok(()),
                    Poll::Ready(result) => current.resolution = Resolution::ReadingParts(result?),
                },
                Resolution::ReadingParts(parts) => {
                    let part = match parts.poll_next_unpin(cx) {
                        Poll::Pending => return Ok(()),
                        Poll::Ready(Some(part)) => part.map_err(Error::StreamRegistry)?,
                        Poll::Ready(None) => {
                            current.finish_resolution()?;
                            return Ok(());
                        }
                    };
                    if let Some(range) = current.select_part(part.metadata.logical_len())? {
                        current.next_file = Some(ReadFile::new(
                            self.file_registry.clone(), self.backend.clone(),
                            part.file_id, part.metadata, range.start, Some(range.end),
                        )?);
                    }
                }
                Resolution::Finished => return Ok(()),
            }
        }
        cx.waker().wake_by_ref();
        Ok(())
    }

    fn finish_current(&mut self) {
        if self.current.as_ref().is_some_and(CurrentStream::consumed) {
            self.current = None;
        }
        let files_admitted = self.current.as_ref().is_none_or(CurrentStream::files_admitted);
        if self.input_closed && files_admitted {
            if let Some(carbon) = &mut self.carbon {
                carbon.close_files();
            }
        }
    }

    fn abort(&mut self) {
        self.input_closed = true;
        self.current = None;
        // Dropping CARBON cancels I/O and releases its shared budget permit.
        self.carbon = None;
    }

    fn fail(&mut self, error: Error) {
        self.abort();
        self.error = Some(error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current(start: u64, end: Option<u64>) -> CurrentStream {
        CurrentStream {
            stream_id: 1,
            start,
            end,
            resolution: Resolution::ReadingParts(Box::pin(futures_util::stream::empty())),
            position_in_stream: 0,
            frames_remaining: 0,
            next_file: None,
        }
    }

    #[test]
    fn ranges_cross_parts_and_completion_waits_for_consumption() -> Result<(), Box<dyn std::error::Error>> {
        let mut request = current(5, Some(10));
        for (length, expected) in [(3, None), (4, Some(2..4)), (5, Some(0..3))] {
            if request.select_part(length)? != expected {
                return Err("incorrect part selection".into());
            }
        }
        request.frames_remaining = 2;
        if !request.files_admitted() || request.consumed() {
            return Err("request completed before its frames were delivered".into());
        }
        request.frames_remaining = 0;
        if !request.consumed() {
            return Err("request did not complete after its frames were delivered".into());
        }
        let mut tail = current(5, None);
        if tail.select_part(3)?.is_some() || tail.select_part(4)? != Some(2..4) {
            return Err("open-ended selection was mapped incorrectly".into());
        }
        tail.finish_resolution()?;
        if !tail.consumed() {
            return Err("open-ended resolution did not finish".into());
        }
        let mut beyond_eof = current(5, Some(10));
        beyond_eof.select_part(7)?;
        if beyond_eof.finish_resolution().is_ok() {
            return Err("a range beyond EOF was silently truncated".into());
        }
        let mut empty = current(7, Some(7));
        if empty.select_part(7)?.is_some() || !empty.consumed() {
            return Err("an empty EOF selection was mishandled".into());
        }
        Ok(())
    }
}
