use std::{
    future::{Future, poll_fn},
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use carbon_io::{AsyncFilesWrite, BytesFrameAllocator, FrameBudget, SchedulerConfig, WriteScheduler};
use futures_io::AsyncWrite;
use stream_registry::{StreamId, StreamPartId, traits::StreamRegistry as _};

use crate::{Error, FileBackend, FileRegistry, StreamRegistry, StreamWriterConfig, WriteFile};

type ManifestFuture = Pin<Box<dyn Future<Output = Result<StreamId, Error>>>>;

enum State {
    Writing,
    Closing,
    StoringManifest(ManifestFuture),
    Closed(StreamId),
    Failed,
}

/// Stream writer borrowing CARBON resources owned by the caller.
///
/// Closing drains CARBON and stores the ordered stream manifest. Flushing is
/// a no-op. Work advances only while the writer is polled; dropping it does not
/// finalize the stream.
pub struct WriteStream<'a> {
    scheduler: WriteScheduler<'a, BytesFrameAllocator, WriteFile>,
    file_registry: FileRegistry,
    stream_registry: StreamRegistry,
    backend: FileBackend,
    config: StreamWriterConfig,
    parts: Vec<StreamPartId>,
    completed_files: usize,
    state: State,
}

impl<'a> WriteStream<'a> {
    /// Initializes CARBON and fills its window with unopened file descriptors.
    ///
    /// No file IDs or backend paths are allocated here. The allocator must use
    /// the fixed frame size specified in `config`. The allocator, shared budget,
    /// and CARBON configuration remain borrowed for the writer's lifetime.
    pub fn new(
        file_registry: FileRegistry,
        stream_registry: StreamRegistry,
        backend: FileBackend,
        config: StreamWriterConfig,
        allocator: &'a mut BytesFrameAllocator,
        budget: &'a FrameBudget,
        carbon_config: &'a mut SchedulerConfig,
    ) -> Result<Self, Error> {
        let mut scheduler = WriteScheduler::new(allocator, budget, carbon_config)
            .map_err(Error::Carbon)?;
        // Populate the window before writing; later replacements use pop_swap.
        while scheduler.accept_file() {
            let file = WriteFile::new(
                file_registry.clone(), stream_registry.clone(), backend.clone(), config,
            )?;
            scheduler.push_file(file).map_err(Error::from)?;
        }
        Ok(Self {
            scheduler,
            file_registry,
            stream_registry,
            backend,
            config,
            parts: Vec::new(),
            completed_files: 0,
            state: State::Writing,
        })
    }

    /// Returns the persisted stream ID after a successful close.
    pub fn stream_id(&self) -> Option<StreamId> {
        match self.state {
            State::Closed(id) => Some(id),
            _ => None,
        }
    }

    /// Advances file writes and part registration without supplying bytes.
    ///
    /// Remains pending while the stream is open and returns only on error.
    /// Cancelling this future leaves the ongoing operations in the writer,
    /// so it can be used alongside an input read in a `select!` loop.
    pub async fn progress(&mut self) -> io::Result<()> {
        poll_fn(|cx| {
            if let Poll::Ready(Err(error)) = Pin::new(&mut *self).poll_write(cx, &[]) {
                return Poll::Ready(Err(error));
            }
            // ponytail: drain newly completed files before waiting for a wakeup.
            if let Err(error) = self.collect_parts() {
                self.state = State::Failed;
                return Poll::Ready(Err(io::Error::other(error)));
            }
            Poll::Pending
        }).await
    }

    fn new_file(&self) -> Result<WriteFile, Error> {
        WriteFile::new(
            self.file_registry.clone(), self.stream_registry.clone(),
            self.backend.clone(), self.config,
        )
    }

    fn collect_parts(&mut self) -> Result<(), Error> {
        while self.completed_files > 0 {
            let (_, part_id) = match self.state {
                State::Writing => {
                    let replacement = self.new_file()?;
                    self.scheduler.pop_swap(replacement).map_err(Error::from)?
                }
                State::Closing => self.scheduler.pop_file().map_err(Error::from)?
                    .ok_or(Error::Carbon(carbon_io::ContractError::AdmissionNotReady))?,
                _ => return Err(Error::Carbon(carbon_io::ContractError::InputClosed)),
            };
            self.parts.push(part_id);
            self.completed_files = self.completed_files.saturating_sub(1);
        }
        Ok(())
    }

    fn poll_input(&mut self, cx: &mut Context<'_>, input: &[u8]) -> Poll<Result<usize, Error>> {
        if !matches!(self.state, State::Writing) {
            return Poll::Ready(Err(Error::Carbon(carbon_io::ContractError::InputClosed)));
        }
        loop {
            self.collect_parts()?;
            let status = ready!(Pin::new(&mut self.scheduler).poll_write(cx, input))
                .map_err(Error::from)?;
            self.completed_files = status.completed_files;
            if let Some(count) = status.output {
                // Acknowledge consumed bytes before any more fallible work.
                return Poll::Ready(Ok(count));
            }
        }
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<StreamId, Error>> {
        if matches!(self.state, State::Writing) {
            self.state = State::Closing;
        }
        loop {
            match &mut self.state {
                State::Closing => {
                    let count = ready!(Pin::new(&mut self.scheduler).poll_close(cx))
                        .map_err(Error::from)?;
                    self.completed_files = count;
                    self.collect_parts()?;
                    if count == 0 {
                        let parts = std::mem::take(&mut self.parts);
                        let registry = self.stream_registry.clone();
                        self.state = State::StoringManifest(Box::pin(async move {
                            registry.store(parts).await.map_err(Error::StreamRegistry)
                        }));
                    }
                }
                State::StoringManifest(future) => {
                    let id = ready!(future.as_mut().poll(cx))?;
                    self.state = State::Closed(id);
                    return Poll::Ready(Ok(id));
                }
                State::Closed(id) => return Poll::Ready(Ok(*id)),
                State::Writing | State::Failed => {
                    return Poll::Ready(Err(Error::Carbon(carbon_io::ContractError::InputClosed)));
                }
            }
        }
    }
}

impl AsyncWrite for WriteStream<'_> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match this.poll_input(cx, input) {
            Poll::Ready(Err(error)) => {
                if matches!(this.state, State::Writing) {
                    this.state = State::Failed;
                }
                Poll::Ready(Err(io::Error::other(error)))
            }
            Poll::Ready(Ok(count)) => Poll::Ready(Ok(count)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match this.poll_finish(cx) {
            Poll::Ready(Err(error)) => {
                this.state = State::Failed;
                Poll::Ready(Err(io::Error::other(error)))
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}
