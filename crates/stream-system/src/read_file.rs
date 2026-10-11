use std::{
    future::Future,
    io,
    ops::Range,
    pin::Pin,
    task::{Context, Poll, ready},
};

use backends::traits::FileBackend as _;
use craft_codec::{Decoder, FrameIter, FrameRead, Metadata};
use file_registry::{FileId, ReadAccessPath, traits::FileRegistry as _};
use futures_io::AsyncRead;
use futures_util::StreamExt;

use crate::{Error, FileBackend, FileRegistry};

type OpenFuture = Pin<Box<dyn Future<Output = Result<(backends::ReadFile, Decoder), Error>>>>;

enum State {
    Closed,
    Opening(OpenFuture),
    Opened { file: backends::ReadFile, decoder: Decoder },
    Failed,
}

/// Complete decoded frame and the requested half-open selection within it.
///
/// CARBON exchanges this buffer with the caller without copying its contents.
/// The selection becomes available only when a complete frame has been decoded.
#[derive(Debug, Default)]
pub struct ReadFrame {
    pub buffer: Vec<u8>,
    pub selected: Range<usize>,
}

/// Reads one stream part at a logical range relative to its file.
///
/// Stream offsets must first be resolved to a part and a local offset by the
/// caller. Construction prepares the physical range without registry or backend
/// I/O. CARBON supplies reusable frame buffers. Each poll performs at most one
/// backend read; pending polls retain the frame descriptor and byte offset.
pub struct ReadFile<'a> {
    file_registry: FileRegistry,
    file_backend: FileBackend,
    file_id: FileId,
    metadata: &'a Metadata,
    frames: FrameIter<'a>,
    logical_range: Range<u64>,
    physical_start: usize,
    physical_end: Option<usize>,
    frame_count: u32,
    current: Option<FrameRead>,
    read: usize,
    state: State,
}

impl<'a> ReadFile<'a> {
    /// Selects logical `[start, end)` bytes; `None` means the end of this part.
    ///
    /// CRAFT expands the selection to complete stored frames. Empty selections
    /// are rejected because CARBON requires a nonzero frame count; the stream
    /// reader must skip them. The caller retains the metadata; its frame iterator
    /// is kept for incremental reading without collecting descriptors.
    pub fn new(
        file_registry: FileRegistry,
        file_backend: FileBackend,
        file_id: FileId,
        metadata: &'a Metadata,
        start: u64,
        end: Option<u64>,
    ) -> Result<Self, Error> {
        let (physical_start, physical_end, frame_count, frames) =
            prepare_range(metadata, start, end)?;
        let logical_end = end.unwrap_or(metadata.logical_len());
        Ok(Self {
            file_registry,
            file_backend,
            file_id,
            metadata,
            frames,
            logical_range: start..logical_end,
            physical_start,
            physical_end,
            frame_count,
            current: None,
            read: 0,
            state: State::Closed,
        })
    }

    /// Requested plaintext range relative to this part, excluding its end.
    pub fn logical_range(&self) -> &Range<u64> {
        &self.logical_range
    }
}

impl carbon_io::ReadFile<ReadFrame> for ReadFile<'_> {
    type Error = Error;

    /// Exact number of complete frames covering the selection, before opening.
    fn frame_count(&self) -> u32 {
        self.frame_count
    }

    /// Resolves the file access path and opens its selected physical range.
    ///
    /// Pending polls retain the same future. An opening error resets the state
    /// so the next call can resolve a fresh path and retry. An already opened
    /// file succeeds immediately. Dropping the descriptor cancels its operation.
    fn poll_open(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let this = self.get_mut();
        loop {
            match &mut this.state {
                State::Closed => {
                    let decoder = Decoder::try_from(this.metadata).map_err(Error::Codec)?;
                    let registry = this.file_registry.clone();
                    let backend = this.file_backend.clone();
                    let id = this.file_id;
                    let start = this.physical_start;
                    let end = this.physical_end;
                    // ponytail: one owned future is polled directly, without a task.
                    this.state = State::Opening(Box::pin(async move {
                        let path = retrieve_path(&registry, id).await?;
                        let file = backend.read(&path.access_path, start, end)
                            .await.map_err(Error::Backend)?;
                        Ok((file, decoder))
                    }));
                }
                State::Opening(future) => {
                    return match ready!(future.as_mut().poll(cx)) {
                        Ok((file, decoder)) => {
                            this.state = State::Opened { file, decoder };
                            Poll::Ready(Ok(()))
                        }
                        Err(error) => {
                            this.state = State::Closed;
                            Poll::Ready(Err(error))
                        }
                    };
                }
                State::Opened { .. } => return Poll::Ready(Ok(())),
                State::Failed => return Poll::Ready(Err(Error::FileNotOpen)),
            }
        }
    }

    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        frame: &mut ReadFrame,
    ) -> Poll<Result<Option<()>, Error>> {
        let this = self.get_mut();
        let State::Opened { file, decoder } = &mut this.state else {
            return Poll::Ready(Err(Error::FileNotOpen));
        };
        let result = (|| {
            if this.current.is_none() {
                let Some(descriptor) = this.frames.next() else {
                    return Poll::Ready(Ok(None));
                };
                let length = descriptor.spec.stored_len();
                if length > isize::MAX as usize {
                    return Poll::Ready(Err(Error::Codec(craft_codec::Error::Overflow)));
                }
                frame.buffer.clear();
                frame.selected = 0..0;
                frame.buffer.try_reserve(length)
                    .map_err(|_| Error::Codec(craft_codec::Error::Allocation))?;
                frame.buffer.resize(length, 0);
                this.current = Some(descriptor);
                this.read = 0;
            }

            let remaining = frame.buffer.get_mut(this.read..)
                .ok_or(Error::Codec(craft_codec::Error::InvalidStoredLength))?;
            let available = remaining.len();
            let count = ready!(Pin::new(&mut *file).poll_read(cx, remaining))
                .map_err(Error::Io)?;
            if count == 0 {
                return Poll::Ready(Err(Error::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "backend ended before the complete stored frame was read",
                ))));
            }
            if count > available {
                return Poll::Ready(Err(Error::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "backend reported more read bytes than the buffer can hold",
                ))));
            }
            this.read = this.read.checked_add(count)
                .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
            if this.read < frame.buffer.len() {
                // ponytail: yield after one backend read so other files can advance.
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }

            let descriptor = this.current.take()
                .ok_or(Error::Codec(craft_codec::Error::InvalidMetadata))?;
            decoder.decode_frame(descriptor.spec, &mut frame.buffer)
                .map_err(Error::Codec)?;
            frame.selected = descriptor.selected;
            this.read = 0;
            Poll::Ready(Ok(Some(())))
        })();
        if matches!(&result, Poll::Ready(Err(_))) {
            this.state = State::Failed;
            this.current = None;
            this.read = 0;
            frame.buffer.clear();
            frame.selected = 0..0;
        }
        result
    }
}

fn prepare_range(
    metadata: &Metadata,
    start: u64,
    end: Option<u64>,
) -> Result<(usize, Option<usize>, u32, FrameIter<'_>), Error> {
    let (physical, frames) = metadata.range(start, end).map_err(Error::Codec)?;
    let count = u32::try_from(frames.remaining())
        .map_err(|_| Error::Codec(craft_codec::Error::Overflow))?;
    if count == 0 {
        return Err(Error::Carbon(carbon_io::ContractError::ZeroFrameCount));
    }
    let physical_start = usize::try_from(physical.start)
        .map_err(|_| Error::Codec(craft_codec::Error::Overflow))?;
    // The current backend uses an inclusive end. A selection reaching file EOF
    // uses its existing open-ended range instead.
    let physical_end = if physical.end == metadata.stored_len() {
        None
    } else {
        let last = physical.end.checked_sub(1)
            .ok_or(Error::Codec(craft_codec::Error::InvalidRange))?;
        Some(usize::try_from(last)
            .map_err(|_| Error::Codec(craft_codec::Error::Overflow))?)
    };
    Ok((physical_start, physical_end, count, frames))
}

async fn retrieve_path(registry: &FileRegistry, id: FileId) -> Result<ReadAccessPath, Error> {
    let paths = registry.retrieve(&[id]).await.map_err(Error::FileRegistry)?;
    let mut paths = std::pin::pin!(paths);
    paths.next().await
        .ok_or(Error::FileRegistry(file_registry::Error::MissingReadAccessPath(id)))?
        .map_err(Error::FileRegistry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_codec::{Compression, Config, Encryption, FrameSizes, Framing};

    #[test]
    fn local_offsets_select_complete_stored_frames() -> Result<(), Box<dyn std::error::Error>> {
        for compression in [Compression::None, Compression::Lz4] {
            for encryption in [Encryption::None, Encryption::Aes256Gcm { key: [0; 32] }] {
                let tag = encryption.tag_len();
                let payload = if compression == Compression::None { 4 } else { 2 };
                let mut metadata = Metadata::new(Config::new(
                    Framing::Fixed(4), compression, encryption,
                )?);
                for _ in 0..3 {
                    metadata.push(FrameSizes::new(4, payload)?)?;
                }
                let stride = usize::try_from(payload)?.checked_add(tag)
                    .ok_or("stored frame size overflow")?;
                let second_end = stride.checked_mul(2).and_then(|end| end.checked_sub(1))
                    .ok_or("stored range overflow")?;
                for (start, end, expected, selected) in [
                    (5, Some(6), (stride, Some(second_end), 1), 1..2),
                    (4, Some(8), (stride, Some(second_end), 1), 0..4),
                    (5, Some(9), (stride, None, 2), 1..4),
                    (5, None, (stride, None, 2), 1..4),
                ] {
                    let (physical_start, physical_end, count, mut frames) =
                        prepare_range(&metadata, start, end)?;
                    if (physical_start, physical_end, count) != expected
                        || frames.remaining() != u64::from(count)
                    {
                        return Err("logical offsets did not select the expected stored frames".into());
                    }
                    let first = frames.next().ok_or("selected frame iterator was lost")?;
                    if first.spec.index() != 1 || first.selected != selected {
                        return Err("frame iterator lost its original index or selection".into());
                    }
                }
                if !matches!(prepare_range(&metadata, 6, Some(6)),
                    Err(Error::Carbon(carbon_io::ContractError::ZeroFrameCount)))
                    || !matches!(prepare_range(&metadata, 5, Some(4)),
                        Err(Error::Codec(craft_codec::Error::InvalidRange)))
                    || !matches!(prepare_range(&metadata, 13, None),
                        Err(Error::Codec(craft_codec::Error::InvalidRange)))
                {
                    return Err("empty or invalid selections were not rejected".into());
                }
            }
        }
        Ok(())
    }
}
