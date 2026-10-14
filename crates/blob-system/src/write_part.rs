use std::{
    future::{Future, poll_fn},
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use blob_registry::traits::BlobRegistry as _;
use futures_io::AsyncWrite;
use stream_system::WriteStream;

use crate::{
    BlobId, BlobRegistry, BlobSystem, BlobWriterConfig, BytesFrameAllocator, Error, FrameBudget,
    PartId, SchedulerConfig,
};

type SaveFuture = Pin<Box<dyn Future<Output = Result<(), Error>> + Send>>;

enum State {
    Writing,
    Closing,
    Saving(SaveFuture),
    Closed,
    Failed,
}

/// An AsyncWrite part backed directly by stream-system's WriteStream.
/// Successful close publishes the stream and then saves its reference in the
/// blob registry. Neither the stream writer nor its ID is exposed to callers.
/// The allocator must have the fixed frame size from BlobWriterConfig.
/// Drop this writer before reusing its borrowed CARBON resources.
pub struct WriteBlobPart<'a> {
    stream: Option<WriteStream<'a>>,
    registry: BlobRegistry,
    blob_id: BlobId,
    part_id: PartId,
    length: u64,
    state: State,
}

impl<'a> WriteBlobPart<'a> {
    /// Checks can_save before constructing the internal WriteStream.
    /// Allocates no backend file or file ID. Actual allocation stays lazy in
    /// WriteStream. The system is not borrowed by the returned writer.
    pub async fn new(
        system: &BlobSystem,
        blob_id: BlobId,
        part_id: PartId,
        config: BlobWriterConfig,
        allocator: &'a mut BytesFrameAllocator,
        budget: &'a FrameBudget,
        carbon_config: &'a mut SchedulerConfig,
    ) -> Result<Self, Error> {
        system.can_save(blob_id).await?;
        let stream = WriteStream::new(
            system.file_registry.clone(),
            system.stream_registry.clone(),
            system.backend.clone(),
            config,
            allocator,
            budget,
            carbon_config,
        )?;
        Ok(Self {
            stream: Some(stream),
            registry: system.blob_registry.clone(),
            blob_id,
            part_id,
            length: 0,
            state: State::Writing,
        })
    }

    pub fn blob_id(&self) -> BlobId {
        self.blob_id
    }

    pub fn part_id(&self) -> PartId {
        self.part_id
    }

    /// Plaintext bytes accepted so far; complete part length after closing.
    pub fn length(&self) -> u64 {
        self.length
    }

    /// True only after both stream closing and blob part registration succeed.
    pub fn is_closed(&self) -> bool {
        matches!(self.state, State::Closed)
    }

    /// Drives the internal WriteStream without accepting additional bytes.
    /// Remains pending while writing; reports I/O errors. Cancelling this
    /// future preserves ongoing operations. It does not close the part.
    pub async fn progress(&mut self) -> io::Result<()> {
        if !matches!(self.state, State::Writing) {
            return Err(Error::InputClosed.into());
        }
        let result = match &mut self.stream {
            Some(stream) => stream.progress().await,
            None => Err(Error::InputClosed.into()),
        };
        if result.is_err() {
            self.fail();
        }
        result
    }

    /// Awaitable close; has the same semantics as AsyncWrite::poll_close.
    pub async fn close(&mut self) -> io::Result<()> {
        poll_fn(|cx| Pin::new(&mut *self).poll_close(cx)).await
    }

    fn poll_input(&mut self, cx: &mut Context<'_>, input: &[u8]) -> Poll<io::Result<usize>> {
        if !matches!(self.state, State::Writing) {
            return Poll::Ready(Err(Error::InputClosed.into()));
        }
        // Validate before accepting bytes so a length error consumes no input.
        let input_length = u64::try_from(input.len()).map_err(|_| Error::LengthOverflow)?;
        self.length
            .checked_add(input_length)
            .ok_or(Error::LengthOverflow)?;
        let stream = self.stream.as_mut().ok_or(Error::InputClosed)?;
        let count = ready!(Pin::new(stream).poll_write(cx, input))?;
        let accepted = u64::try_from(count).map_err(|_| Error::LengthOverflow)?;
        self.length = self
            .length
            .checked_add(accepted)
            .ok_or(Error::LengthOverflow)?;
        Poll::Ready(Ok(count))
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if matches!(self.state, State::Writing) {
            self.state = State::Closing;
        }
        loop {
            match &mut self.state {
                State::Closing => {
                    let stream = self.stream.as_mut().ok_or(Error::InputClosed)?;
                    ready!(Pin::new(&mut *stream).poll_close(cx))?;
                    let stream_id = stream.stream_id().ok_or(Error::MissingStreamId)?;
                    let registry = self.registry.clone();
                    let blob_id = self.blob_id;
                    let part_id = self.part_id;
                    let length = self.length;
                    self.stream = None;
                    self.state = State::Saving(Box::pin(async move {
                        registry
                            .save_part(blob_id, stream_id, length, part_id)
                            .await?;
                        Ok(())
                    }));
                }
                State::Saving(future) => {
                    ready!(future.as_mut().poll(cx))?;
                    self.state = State::Closed;
                    return Poll::Ready(Ok(()));
                }
                State::Closed => return Poll::Ready(Ok(())),
                State::Writing | State::Failed => {
                    return Poll::Ready(Err(Error::InputClosed.into()));
                }
            }
        }
    }

    fn fail(&mut self) {
        self.state = State::Failed;
        self.stream = None;
    }
}

impl AsyncWrite for WriteBlobPart<'_> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = this.poll_input(cx, input);
        if matches!(&result, Poll::Ready(Err(_))) && matches!(this.state, State::Writing) {
            this.fail();
        }
        result
    }

    /// Flushing does not finalize the stream or register the part.
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    /// Finishes WriteStream, then atomically saves the part and its byte length.
    /// Pending retains both closing and registration futures. Successful close
    /// is idempotent. A failure is terminal. Dropping does not run close or
    /// roll back a registration that has already committed.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = this.poll_finish(cx);
        if matches!(&result, Poll::Ready(Err(_))) {
            this.fail();
        }
        result
    }
}
