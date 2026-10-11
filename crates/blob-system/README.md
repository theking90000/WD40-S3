# blob-system

Write multipart blobs and read their logical byte ranges through blob and part
IDs. Part writers use `stream-system::WriteStream`; readers use its `ReadStream`.
Stream IDs and their lifecycle stay inside the adapters.

Create a cloneable `BlobSystem` with the existing registry and backend handles.
The system owns no runtime or CARBON resources.

```rust
use blob_system::{BlobSystem, BlobWriterConfig, Compression, Encryption};
use futures_util::io::{AsyncReadExt, AsyncWriteExt};

let system = BlobSystem::new(blob_registry, file_registry, stream_registry, backend);
let blob_id = system.create().await?;
let settings = BlobWriterConfig {
    encryption: Encryption::Aes256,
    compression: Compression::Lz4,
    frame_size: 4096,
    max_file_size: 1024 * 1024,
};

let mut part = system.write_part(
    blob_id, 1, settings, &mut allocator, &budget, &mut carbon_config,
).await?;
part.write_all(payload).await?;
part.close().await?; // Closes WriteStream, then registers the blob part.
drop(part); // Allows reuse of the borrowed CARBON resources.

let length = system.finalize(blob_id, vec![1]).await?;
let mut reader = system.read(blob_id, 0..length, &budget, &mut carbon_config)?
    .into_async_read();
let mut bytes = Vec::new();
reader.read_to_end(&mut bytes).await?;
```

The allocator, payload and storage/CARBON handles in the example belong to the
application. Enable `futures-util`'s `io` feature to use the byte I/O extensions.
The allocator must use the fixed plaintext frame size from `BlobWriterConfig`.
Readers and writers own cloned handles and borrow only their CARBON resources.
Different concurrent operations need separate allocators/configurations and may
share a frame budget.

Each part writer counts the plaintext bytes accepted by `WriteStream`. Closing
waits for stream publication, then saves its ID and length in `BlobRegistry`.
Repeated successful closing is harmless. Flushing does not finalize anything.
`progress()` drives writing while an independent input source is awaited.
Cancelled write/close futures retain their operations in the writer; dropping
the writer cancels local work without rolling back registered data.

`write_part` awaits `can_save` before creating the underlying writer, so a
missing/finalized blob or existing part is rejected before backend allocation.
The check reserves nothing. Concurrent finalization or another upload of the
same part ID can still make closing fail after its stream was published. Such
streams remain registered. A storage error can also occur after a commit; a
failed close is terminal and does not imply rollback.

Close every selected part writer before calling `finalize`. Its explicit part
ordering defines the blob; repetitions and empty selections are allowed. The
operation returns the total length and freezes the blob.

`read` constructs a lazy `ReadBlob`. It resolves no metadata and opens no file
until polled. Missing/unfinalized blobs and invalid ranges are reported during
reading. Byte ranges have an exclusive end. The blob registry supplies slices
progressively; the reader finishes each slice before requesting the next.
CARBON retains concurrency between files in the current stream.

For frame access without payload copying:

```rust
use blob_system::ReadFrame;

let mut reader = system.read(blob_id, start..end, &budget, &mut carbon_config)?;
let mut frame = ReadFrame::default();
while reader.read_frame(&mut frame).await?.is_some() {
    let bytes = frame.buffer.get(frame.selected.clone())
        .ok_or_else(|| std::io::Error::other("invalid frame selection"))?;
    consume(bytes).await?;
}
```

`ReadBlob` also implements `Stream<Item = Result<ReadFrame, Error>>`. Its frame
methods recycle the previous output buffer; the Stream API delivers owned
frames. `into_async_read` copies selected bytes into the destination and retains
a partially consumed frame between calls. A terminal read error cancels pending
operations and is emitted once, followed by EOF. Reader `progress()` never
completes and does not consume frames; errors are retained for the next read.
