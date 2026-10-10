use carbon_io::{AsyncFilesWrite, BytesFrameAllocator, FrameBudget, SchedulerConfig, WriteScheduler};

use crate::{Error, FileBackend, FileRegistry, StreamRegistry, StreamWriterConfig, WriteFile};

/// Stream writer borrowing CARBON resources owned by the caller.
///
/// This increment initializes the destination window only; writing and finalization follow.
#[expect(dead_code, reason = "this increment implements stream construction only")]
pub struct WriteStream<'a> {
    scheduler: WriteScheduler<'a, BytesFrameAllocator, WriteFile>,
    file_registry: FileRegistry,
    stream_registry: StreamRegistry,
    backend: FileBackend,
    config: StreamWriterConfig,
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
        while scheduler.accept_file() {
            let file = WriteFile::new(file_registry.clone(), backend.clone(), config)?;
            scheduler.push_file(file).map_err(Error::from)?;
        }
        Ok(Self {
            scheduler,
            file_registry,
            stream_registry,
            backend,
            config,
        })
    }
}
