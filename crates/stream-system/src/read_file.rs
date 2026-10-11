use std::{
    future::Future,
    io,
    ops::Range,
    pin::Pin,
    task::{Context, Poll, ready},
};

use backends::traits::FileBackend as _;
use craft_codec::Metadata;
use file_registry::{FileId, ReadAccessPath, traits::FileRegistry as _};
use futures_util::StreamExt;
use stream_registry::StreamPart;

use crate::{Error, FileBackend, FileRegistry};

type OpenFuture = Pin<Box<dyn Future<Output = Result<backends::ReadFile, Error>>>>;

enum State {
    Closed,
    Opening(OpenFuture),
    Opened(backends::ReadFile),
}

/// Opens one stream part at a logical range relative to its file.
///
/// Stream offsets must first be resolved to a part and a local offset by the
/// caller. Construction prepares the physical range without registry or backend
/// I/O. CARBON's opening methods are implemented; frame reads currently return
/// an unsupported-operation error until the next increment.
pub struct ReadFile {
    file_registry: FileRegistry,
    file_backend: FileBackend,
    part: StreamPart,
    logical_range: Range<u64>,
    physical_start: usize,
    physical_end: Option<usize>,
    frame_count: u32,
    state: State,
}

impl ReadFile {
    /// Selects logical `[start, end)` bytes; `None` means the end of this part.
    ///
    /// CRAFT expands the selection to complete stored frames. Empty selections
    /// are rejected because CARBON requires a nonzero frame count; the stream
    /// reader must skip them. Metadata is retained without cloning it.
    pub fn new(
        file_registry: FileRegistry,
        file_backend: FileBackend,
        part: StreamPart,
        start: u64,
        end: Option<u64>,
    ) -> Result<Self, Error> {
        let (physical_start, physical_end, frame_count) =
            prepare_range(&part.metadata, start, end)?;
        let logical_end = end.unwrap_or(part.metadata.logical_len());
        Ok(Self {
            file_registry,
            file_backend,
            part,
            logical_range: start..logical_end,
            physical_start,
            physical_end,
            frame_count,
            state: State::Closed,
        })
    }

    /// Requested plaintext range relative to this part, excluding its end.
    pub fn logical_range(&self) -> &Range<u64> {
        &self.logical_range
    }
}

impl carbon_io::ReadFile<Vec<u8>> for ReadFile {
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
                    let registry = this.file_registry.clone();
                    let backend = this.file_backend.clone();
                    let id = this.part.file_id;
                    let start = this.physical_start;
                    let end = this.physical_end;
                    // ponytail: one owned future is polled directly, without a task.
                    this.state = State::Opening(Box::pin(async move {
                        let path = retrieve_path(&registry, id).await?;
                        backend.read(&path.access_path, start, end)
                            .await.map_err(Error::Backend)
                    }));
                }
                State::Opening(future) => {
                    return match ready!(future.as_mut().poll(cx)) {
                        Ok(file) => {
                            this.state = State::Opened(file);
                            Poll::Ready(Ok(()))
                        }
                        Err(error) => {
                            this.state = State::Closed;
                            Poll::Ready(Err(error))
                        }
                    };
                }
                State::Opened(_file) => return Poll::Ready(Ok(())),
            }
        }
    }

    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _frame: &mut Vec<u8>,
    ) -> Poll<Result<Option<()>, Error>> {
        Poll::Ready(Err(Error::Io(io::Error::new(
            io::ErrorKind::Unsupported,
            "stream frame reading is not implemented yet",
        ))))
    }
}

fn prepare_range(
    metadata: &Metadata,
    start: u64,
    end: Option<u64>,
) -> Result<(usize, Option<usize>, u32), Error> {
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
    Ok((physical_start, physical_end, count))
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
                if prepare_range(&metadata, 5, Some(6))? != (stride, Some(second_end), 1)
                    || prepare_range(&metadata, 4, Some(8))? != (stride, Some(second_end), 1)
                    || prepare_range(&metadata, 5, Some(9))? != (stride, None, 2)
                    || prepare_range(&metadata, 5, None)? != (stride, None, 2)
                {
                    return Err("logical offsets did not select the expected stored frames".into());
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
