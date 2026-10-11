# Stream and file API guide

The examples are fragments inside application async functions returning a
fallible result. Registry/backend construction, input sources and output
consumers are supplied by the application. Extension-method examples require
`futures-util` with its `io` feature; `select!` examples additionally use Tokio's
`macros` feature. The stream objects implement `futures-io` traits, which are
separate from Tokio's I/O traits.

## Resources and ownership

| Resource | Purpose | Constructor ownership |
| --- | --- | --- |
| `FileRegistry` | Allocate file IDs and resolve/store backend access paths | Owned handle; clone before passing if retained elsewhere |
| `StreamRegistry` | Store manifests and resolve ordered parts with CRAFT metadata | Owned handle |
| `FileBackend` | Open readers/writers and finalize uploaded files | Owned handle |
| `FrameBudget` | Shared CARBON frame capacity, measured in frames | Shared borrow |
| `SchedulerConfig` | Window and retry configuration | Exclusive borrow |
| `BytesFrameAllocator` | Accumulate fixed-size plaintext write frames | Exclusive borrow, writes only |

Each concurrent reader/writer needs its own mutable configuration. A budget can
be shared. Closing an object does not release its Rust borrows: drop the object
before reusing its configuration or write allocator. Resolved read metadata is
owned by the files, not borrowed from the caller.

`StreamSystem` is a bundle of the three registry/backend handles. It performs no
I/O and has no opening methods; construction lives on `ReadStream` and
`WriteStream`.

No stream-system task, mailbox or command queue drives these objects. The stored
operation futures are not required to be `Send`; the objects should not be
assumed to support `tokio::spawn`. A local driver can poll them directly.
`tokio::select!` polls branches in the current task rather than spawning them.
Runtime requirements of the chosen backend still apply; stream-system does not
create an executor or make a backend independent of its networking runtime.

## Open and write a stream

Construction is synchronous and prepares unopened destinations. It does not
allocate file IDs. A destination opens only when data reaches it, using
`FileRegistry::create(1, max_file_size)`. Empty input therefore does not allocate
an otherwise unused window of backend files.

```rust
use futures_util::io::AsyncWriteExt;
use stream_system::{
    BytesFrameAllocator, Compression, Encryption, FrameBudget,
    SchedulerConfig, StreamWriterConfig, WriteStream,
};

let settings = StreamWriterConfig {
    encryption: Encryption::Aes256,
    compression: Compression::Lz4,
    frame_size: 4096,
    max_file_size: 1024 * 1024,
};
let mut allocator = BytesFrameAllocator::new(usize::try_from(settings.frame_size)?)?;
let budget = FrameBudget::new(1024);
let mut carbon_config = SchedulerConfig::default();

let mut writer = WriteStream::new(
    file_registry.clone(), stream_registry.clone(), backend.clone(), settings,
    &mut allocator, &budget, &mut carbon_config,
)?;
writer.write_all(payload).await?;
writer.close().await?;
let id = writer.stream_id()
    .ok_or_else(|| std::io::Error::other("close did not publish a stream ID"))?;
drop(writer);
```

`Encryption::None` disables encryption. `Encryption::Aes256` generates a fresh
system key per file-open attempt, including replay attempts. The key is retained
in the stored CRAFT metadata. Compression is `Compression::None` or
`Compression::Lz4`, independently per frame.

`frame_size` is a plaintext size. `max_file_size` limits encoded file payload,
including authentication tags, rather than registry metadata or the transport
envelope. Capacity is conservative: it uses full frames plus codec overhead;
compression savings do not increase the advertised frame count. The allocator's
frame size must match the settings; the writer does not synchronize those sizes.

### Write lifecycle

| Operation | Meaning |
| --- | --- |
| `new(...)` | Validate configuration and prepare unopened file destinations |
| `write` / `poll_write` | Accept a plaintext prefix and advance scheduled files |
| `write_all` | Continue until the supplied byte slice has been accepted |
| `progress().await` | Advance pending writes and part registration without new bytes; stays pending unless an error occurs |
| `flush` / `poll_flush` | No-op; does not drain files or publish a stream ID |
| `close` / `poll_close` | End input, emit the final short frame, finalize files and store the ordered manifest |
| `stream_id()` | `Some(id)` only in the successfully closed state |
| Drop | Cancel local operations; does not finalize the stream or roll back registered files/parts |

Each file closes through backend finish, file registration and part insertion.
The manifest is stored after all parts complete, in their logical order.
Accepted bytes alone do not mean the manifest has been published. Successful
closing is idempotent. Writing after closing begins is rejected. Cancelling the
close future preserves finalization state so closing can be resumed.

### Wait for input while writes advance

For a streaming input source, replace the one-shot write/close sequence with this
loop after constructing the writer:

```rust
use futures_util::io::{AsyncReadExt, AsyncWriteExt};

let mut buffer = [0u8; 8192];
loop {
    tokio::select! {
        result = writer.progress() => result?,
        count = input.read(&mut buffer) => {
            let count = count?;
            if count == 0 {
                break;
            }
            let bytes = buffer.get(..count)
                .ok_or_else(|| std::io::Error::other("invalid input read count"))?;
            writer.write_all(bytes).await?;
        }
    }
}
writer.close().await?;
let id = writer.stream_id();
```

The losing `progress()` future is dropped without losing writer state. Once the
input branch wins, its progress borrow is released before `write_all` starts.
`write_all` is awaited outside the race; cancelling an arbitrary `write_all`
future can leave an accepted prefix whose length is no longer tracked by that
extension future. The application controls cancellation of the overall upload.

While neither writing nor progression is polled, stream-system does not advance
its pending operations. `progress()` is for the writing phase and does not close
the writer.

## Open and push read requests

```rust
use stream_system::{ReadFrame, ReadStream};

let mut reader = ReadStream::new(
    file_registry, stream_registry, backend, &budget, &mut carbon_config,
)?;
reader.push_stream(id, 100, Some(500))?;
reader.close_stream();

let mut frame = ReadFrame::default();
while reader.read_frame(&mut frame).await?.is_some() {
    let bytes = frame.buffer.get(frame.selected.clone())
        .ok_or_else(|| std::io::Error::other("invalid frame selection"))?;
    consume(bytes).await?;
}
```

The selection is `[100, 500)` in decoded bytes across all parts of this stream.
`None` as the end selects through stream EOF. `new` performs no lookup or backend
opening. `push_stream` installs a stored registry-lookup future; polling resolves
parts incrementally, translates stream positions to local file ranges, and
admits files into CARBON. Parts before the offset are resolved for their lengths
but their backend files are not opened.

The reader holds one current request. `can_push_stream()` becomes true only
after resolution ends and every selected frame has been delivered. Completion
of discovery alone does not free request admission. Selections pushed over time
are concatenated in push order. `close_stream()` prevents further pushes while
allowing the current selection to drain; it is idempotent and is not backend
upload-style finalization.

### Read outcomes and validation

| Operation/outcome | Meaning |
| --- | --- |
| `can_push_stream() == true` | Initially available, or the previous selection has been fully delivered; input remains open |
| `push_stream` returns `ReadStreamPending(id)` | Current request still active; rejection keeps it intact |
| `poll_read_frame` returns `Some(())` | Output buffer exchanged for one complete decoded frame |
| `poll_read_frame` returns `Pending` | Discovery/I/O waits, or input is open with no current request |
| `poll_read_frame` returns `None` | Normal sealed EOF, or EOF after a reported terminal failure |
| `poll_progress` / `progress()` | Drive discovery and I/O without consuming output; `progress()` never completes |
| `poll_can_push_stream` | Drive readiness without consuming frames; closed input is an error |

Reversed ranges are rejected by `push_stream`. Bounds beyond stream EOF are
checked during discovery and can be reported after a valid prefix has already
been delivered. Missing manifests/parts are also polling errors. An empty range
still establishes existence and validates its offset, but opens no backend file.

Reader failures cancel local operations and release its CARBON budget. A
terminal failure is surfaced once, then frame reads return EOF. A resolution
failure may be observed by `poll_can_push_stream` instead; backend failures are
observed by frame reads. `progress()` itself never returns an error or EOF.

Push and close mutations do not wake a task. Poll after mutating input. Empty
open input registers no request-input waker. Discovery being pending does not
prevent already admitted files from progressing; the discovery work per poll is
bounded. Delivery remains ordered, so a slow earlier frame still delays output
of later frames.

### Guard request input in select!

A guarded request source must not consume its next ID while a request is active.
The guard is computed before the futures are created and remains fixed for that
invocation of `select!`. A disabled `StreamExt::next` branch is not polled and
does not yield/consume a request, although its future expression is evaluated.

When the current request finishes without another frame, including an empty
selection, the reading branch must complete so the loop can recompute its guard.
The following is an application-local helper, not an exported library method:

```rust
use std::{future::poll_fn, task::Poll};
use stream_system::{Error, ReadFrame, ReadStream};

enum ReadEvent {
    Frame,
    StreamConsumed,
    Eof,
}

async fn next_read_event(
    reader: &mut ReadStream<'_>,
    frame: &mut ReadFrame,
) -> Result<ReadEvent, Error> {
    let had_current = !reader.can_push_stream();
    poll_fn(|cx| {
        match reader.poll_read_frame(cx, frame) {
            Poll::Ready(Ok(Some(()))) => Poll::Ready(Ok(ReadEvent::Frame)),
            Poll::Ready(Ok(None)) => Poll::Ready(Ok(ReadEvent::Eof)),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending if had_current && reader.can_push_stream() => {
                Poll::Ready(Ok(ReadEvent::StreamConsumed))
            }
            Poll::Pending => Poll::Pending,
        }
    }).await
}
```

It detects the transition to available admission only when a request was
previously current. An already idle reader stays pending, avoiding an immediate
completion loop while the external source has no request ready.

```rust
use futures_util::StreamExt;

let mut frame = ReadFrame::default();
loop {
    let can_push = reader.can_push_stream();
    tokio::select! {
        request = requests.next(), if can_push => {
            match request {
                Some((id, start, end)) => reader.push_stream(id, start, end)?,
                None => reader.close_stream(),
            }
        }
        event = next_read_event(&mut reader, &mut frame) => {
            match event? {
                ReadEvent::Frame => {
                    let bytes = frame.buffer.get(frame.selected.clone())
                        .ok_or_else(|| std::io::Error::other("invalid frame selection"))?;
                    consume(bytes).await?;
                }
                ReadEvent::StreamConsumed => continue,
                ReadEvent::Eof => break,
            }
        }
    }
}
```

The reading branch already drives CARBON. A separate `reader.progress()` branch
would attempt a second simultaneous mutable borrow of the same reader.
Cancelling `next_read_event` when a request branch wins preserves all pending
reader operations. Returning `StreamConsumed` exits the current `select!`;
changing the predicate while its reading branch remains pending is insufficient
to enable the previously disabled source branch.

## Frame output and AsyncRead adaptation

`ReadFrame::buffer` holds the complete decoded frame. `selected` identifies the
requested half-open slice within it. Boundary frames can contain bytes outside
the request, so processing the entire buffer would exceed the requested range.
Finish using a borrowed slice before reading into the same frame again.

CARBON swaps output buffers with the caller. The next frame read returns the
previous buffer to its recycling pool. A returned frame and any other buffers
retained by the application are outside `FrameBudget`; the budget counts
scheduler frames, not all bytes held by metadata, codecs and consumers.

The byte adapter replaces the frame-consumption loop after pushing and sealing
the final request:

```rust
use futures_util::io::AsyncReadExt;
use stream_system::AsyncReadAdapter;

// The frame reader has already received its final request and been sealed.
let mut bytes_reader = AsyncReadAdapter::new(reader);
let mut destination = [0u8; 8192];
loop {
    let count = bytes_reader.read(&mut destination).await?;
    if count == 0 {
        break;
    }
    let bytes = destination.get(..count)
        .ok_or_else(|| std::io::Error::other("invalid adapter read count"))?;
    consume(bytes).await?;
}
```

This adapter owns its `ReadStream`, retains a partially consumed frame and copies
only selected bytes. It can return a short read after one frame even when the
destination has more room. A zero-length destination returns zero without
driving the reader. `stream_mut()` allows request pushes, sealing and progression;
direct frame consumption through that accessor would bypass the adapter's byte
sequence and must not be mixed with adapter reads.

The underlying reader can become available while its last frame still has
unread bytes in the adapter. Another pushed stream follows those buffered bytes
in the byte output. The frame API does not require this adapter or its copy.

## Individual file adapters

These types implement CARBON traits directly. Their polling methods are trait
methods, not duplicate inherent APIs. `ReadFile` is not itself `AsyncRead`, and
`WriteFile` is not itself the byte-oriented `AsyncWrite` stream interface.

| WriteFile operation | Contract |
| --- | --- |
| `new(registries, backend, settings)` | Validate profile and capacity; no ID, key or backend file allocated |
| `frame_capacity()` | Stable nonzero frame count across attempts |
| `poll_open(cx)` | Allocate one file/access path, create metadata/encoder with a fresh key if encrypted, open backend |
| `poll_write(cx, &Vec<u8>)` | Encode an immutable plaintext frame into a work buffer, retain partial writes, acknowledge only after all stored bytes are accepted |
| `poll_close(cx)` | Backend finish, file access-path store, metadata/part insert; return a stream part ID |

The same logical write frame must be retained across pending polls. It is not
mutated, allowing CARBON to replay it into a fresh file after errors. Retry count
belongs to `SchedulerConfig`; the adapter does not create its own retry task.
Closing with a pending encoded frame is an error. Completed uploads or registry
writes are not rolled back if later finalization fails.

Direct polling uses the CARBON trait, with the caller retaining the plaintext
frame throughout its write:

```rust
use std::{future::poll_fn, pin::Pin};
use carbon_io::WriteFile as _;
use stream_system::WriteFile;

let mut file = WriteFile::new(
    file_registry.clone(), stream_registry.clone(), backend.clone(), settings,
)?;
poll_fn(|cx| Pin::new(&mut file).poll_open(cx)).await?;
poll_fn(|cx| Pin::new(&mut file).poll_write(cx, &plaintext_frame)).await?;
let part_id = poll_fn(|cx| Pin::new(&mut file).poll_close(cx)).await?;
```

`plaintext_frame` is a `Vec<u8>` respecting the fixed-framing profile. This
example closes a single-frame part; a scheduler can write up to the advertised
capacity before closing. Storing a complete stream manifest is a separate
`StreamRegistry::store` operation over the ordered part IDs.

| ReadFile operation | Contract |
| --- | --- |
| `new(registry, backend, file_id, metadata, start, end)` | Take metadata ownership and map a part-local plaintext range to complete stored frames; no I/O |
| `logical_range()` | Original part-local half-open selection |
| `frame_count()` | Exact stable nonzero count, including partially selected boundary frames |
| `poll_open(cx)` | Resolve file access path, initialize decoder and open the selected backend range |
| `poll_read(cx, &mut ReadFrame)` | Incrementally fill the work buffer and decode one complete frame; set its selection |

Read metadata and the cursor move together without an internal reference.
CRAFT range mapping runs once; subsequent descriptors use the original frame
index. No descriptor collection or repeated prefix-range scan is introduced.
Opening failures can be retried with a fresh access path. Reading/decode errors
are terminal; the work buffer is cleared and the backend reader dropped.

The low-level read work buffer may change during pending polls and must remain
the same logical buffer until completion. This differs from `ReadStream` output,
which stays unchanged on pending/EOF/error. CARBON stops reading a file at its
advertised count without requiring an extra EOF poll.

For a part already resolved by the registry:

```rust
use std::{future::poll_fn, pin::Pin};
use carbon_io::ReadFile as _;
use stream_system::{ReadFile, ReadFrame};

let mut file = ReadFile::new(
    file_registry.clone(), backend.clone(), part.file_id, part.metadata,
    local_start, local_end,
)?;
poll_fn(|cx| Pin::new(&mut file).poll_open(cx)).await?;
let mut frame = ReadFrame::default();
while poll_fn(|cx| Pin::new(&mut file).poll_read(cx, &mut frame)).await?.is_some() {
    let bytes = frame.buffer.get(frame.selected.clone())
        .ok_or_else(|| std::io::Error::other("invalid frame selection"))?;
    consume(bytes).await?;
}
```

The local offsets belong to this part. A direct file driver owns retry policy;
the single open await shown here returns an opening error directly rather than
applying CARBON's scheduler retry count.

## Errors

Constructors and frame-oriented reads return `stream_system::Error`. Its variants
preserve codec, registry, backend, scheduler and byte-I/O causes. Lifecycle
variants identify an unopened file, unfinished frame, full destination, too-small
file limit or still-current stream ID.

The `AsyncWrite` writer and `AsyncRead` adapter expose `std::io::Result` and wrap
stream failures with `io::Error::other`. The adapter's own inconsistent-frame
selection check uses `InvalidData`. Successful writes/reads must be acknowledged
before reporting a later failure; a delivered frame is never silently replaced
by an error from subsequent work.
