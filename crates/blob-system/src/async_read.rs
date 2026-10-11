use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use futures_io::AsyncRead;

use crate::{ReadBlob, ReadFrame};

/// Byte-oriented reads over a blob, retaining partially consumed frames.
/// Only this adapter copies plaintext into the caller's destination buffer.
pub struct AsyncReadAdapter<'a> {
    blob: ReadBlob<'a>,
    frame: ReadFrame,
    position: usize,
}

impl<'a> AsyncReadAdapter<'a> {
    /// Takes ownership of the lazy frame reader without polling it.
    pub fn new(blob: ReadBlob<'a>) -> Self {
        Self {
            blob,
            frame: ReadFrame::default(),
            position: 0,
        }
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
            match ready!(this.blob.poll_read_frame(cx, &mut this.frame))
                .map_err(io::Error::other)?
            {
                Some(()) => this.position = this.frame.selected.start,
                None => return Poll::Ready(Ok(0)),
            }
            if this.frame.selected.is_empty() {
                return Poll::Ready(Err(invalid_frame()));
            }
        }
        let remaining = this
            .frame
            .buffer
            .get(this.position..this.frame.selected.end)
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
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid decoded blob frame selection",
    )
}
