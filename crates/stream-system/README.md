# stream-system

Store a byte stream across backend files, then read a logical byte range using
its stream ID. File access paths belong to `FileRegistry`; ordered manifests and
CRAFT metadata belong to `StreamRegistry`. CARBON schedules the file operations.

## Write

`WriteStream` implements `futures_io::AsyncWrite`. Its constructor takes the
three registry/backend handles, codec settings and caller-owned CARBON resources.

```rust
use futures_util::io::AsyncWriteExt;
use stream_system::{Compression, Encryption, StreamWriterConfig, WriteStream};

let settings = StreamWriterConfig {
    encryption: Encryption::Aes256,
    compression: Compression::Lz4,
    frame_size: 4096,
    max_file_size: 1024 * 1024,
};

let mut writer = WriteStream::new(
    file_registry, stream_registry, backend, settings,
    &mut allocator, &budget, &mut carbon_config,
)?;
writer.write_all(payload).await?;
writer.close().await?;
let stream_id = writer.stream_id(); // Some(id) after successful close.
```

The allocator must use the configured plaintext frame size. File allocation and
opening are lazy. Closing finalizes every file and publishes the stream manifest;
flushing is a no-op. `progress()` advances writes while an independent input
source is awaited.

## Read

`ReadStream` owns the resolved metadata and accepts one current request. Offsets
are plaintext byte positions; `end` is exclusive and `None` means stream EOF.

```rust
use stream_system::{ReadFrame, ReadStream};

let mut reader = ReadStream::new(
    file_registry, stream_registry, backend, &budget, &mut carbon_config,
)?;
reader.push_stream(stream_id, start, end)?;
reader.close_stream(); // No further requests; the current selection still drains.

let mut frame = ReadFrame::default();
while reader.read_frame(&mut frame).await?.is_some() {
    let bytes = frame.buffer.get(frame.selected.clone())
        .ok_or_else(|| std::io::Error::other("invalid frame selection"))?;
    consume(bytes).await?;
}
```

These snippets use application-provided handles, resources, input and consumers.
For multiple requests, push the next ID only when `can_push_stream()` is true,
then seal input after the final request. Frame buffers are exchanged without a
payload copy. `AsyncReadAdapter::new(reader)` optionally supplies `AsyncRead`
by copying only the selected bytes into the destination buffer.

## Drive operations

There is no internal task driving a reader or writer. Reads, writes, closing and
`progress()` advance operations when polled. The library uses `futures-io` traits;
extension methods require the application's `futures-util` `io` feature.

The [API and polling guide](docs/guide.md) covers resource setup, guarded
`select!` loops, empty read selections, errors and the individual file adapters.
The read-loop helper in that guide is application-local and uses existing APIs.

The optional CLI is enabled with the `cli` feature. Common registry, pool and
CARBON options work before or after the subcommand and retain their `WD40_*`
environment variables.

```sh
cargo run -p stream-system --features cli -- --help
cargo run -p stream-system --features cli -- upload --help
cargo run -p stream-system --features cli -- download --help
```

Upload a file and print its decimal stream ID. Set `WD40_DISCORD_WEBHOOK_URLS`
or pass `--webhook` for upload:

```sh
cargo run -p stream-system --features cli -- upload --input input.bin
```

Download using the same persistent registries. A webhook is unnecessary for
download; `--bot-token` or `WD40_DISCORD_BOT_TOKENS` can renew expired URLs.
The codec settings are recovered from stored metadata.

```sh
cargo run -p stream-system --features cli -- download 123 --output output.bin
cargo run -p stream-system --features cli -- download 123 --start 100 --end 200 --output range.bin
```

`--start` defaults to zero and `--end` is exclusive; omitting it reads through
EOF. Offsets select plaintext bytes across file boundaries. `--output -`, the
default, writes raw bytes to stdout. An existing output file is truncated;
an I/O or range error can leave partial output. The download options also accept
`WD40_STREAM_ID`, `WD40_OUTPUT`, `WD40_START` and `WD40_END`.
