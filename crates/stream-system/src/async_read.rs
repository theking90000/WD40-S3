use std::{io, pin::Pin, task::{Context, Poll, ready}};

use futures_io::AsyncRead;

use crate::{ReadFrame, ReadStream};

/// Byte-oriented adapter over the frame reader.
///
/// Only this adapter copies selected plaintext into the caller's byte buffer.
/// It retains a partially consumed frame between polls and returns its buffer
/// to CARBON on the next frame exchange. Dropping it cancels its reader.
pub struct AsyncReadAdapter<'a> {
    stream: ReadStream<'a>,
    frame: ReadFrame,
    position: usize,
}

impl<'a> AsyncReadAdapter<'a> {
    pub fn new(stream: ReadStream<'a>) -> Self {
        Self { stream, frame: ReadFrame::default(), position: 0 }
    }

    /// Accesses the reader to push streams, close input or drive progress.
    /// Do not consume frames directly while the adapter holds unread bytes.
    pub fn stream_mut(&mut self) -> &mut ReadStream<'a> {
        &mut self.stream
    }
}

impl AsyncRead for AsyncReadAdapter<'_> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        destination: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if destination.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let this = self.get_mut();
        if this.position == this.frame.selected.end {
            match ready!(this.stream.poll_read_frame(cx, &mut this.frame))
                .map_err(io::Error::other)?
            {
                Some(()) => this.position = this.frame.selected.start,
                None => return Poll::Ready(Ok(0)),
            }
            if this.frame.selected.is_empty() {
                return Poll::Ready(Err(invalid_frame()));
            }
        }
        let remaining = this.frame.buffer.get(this.position..this.frame.selected.end)
            .ok_or_else(invalid_frame)?;
        let count = destination.len().min(remaining.len());
        let source = remaining.get(..count).ok_or_else(invalid_frame)?;
        let output = destination.get_mut(..count).ok_or_else(invalid_frame)?;
        let position = this.position.checked_add(count).ok_or_else(invalid_frame)?;
        output.copy_from_slice(source);
        this.position = position;
        Poll::Ready(Ok(count))
    }
}

fn invalid_frame() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid decoded frame selection")
}
