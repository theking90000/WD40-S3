# blob-registry

Persist a blob as an ordered sequence of immutable stream references. This crate
stores metadata and resolves plaintext byte ranges. The caller checks that each
stream exists, is finalized, and has the supplied logical length.

Like `stream-registry`, it provides a `traits::BlobRegistry` trait, a
`BlobRegistry::Local` enum variant, and a cloneable `LocalRegistry` using a
caller-owned sled database. Records use postcard. IDs are unique within that
database, including after reopening it.

```rust
use blob_registry::{LocalRegistry, traits::BlobRegistry};
use futures_util::StreamExt;

let registry = LocalRegistry::new(database)?;
let blob_id = registry.create().await?;
registry.can_save(blob_id).await?; // Check before allocating/uploading a stream.
registry.save_part(blob_id, first_stream_id, 10, 1).await?;
registry.save_part(blob_id, second_stream_id, 20, 2).await?;
let length = registry.finalize(blob_id, vec![1, 2]).await?; // 30

let mut slices = registry.read(blob_id, 5..25).await?;
while let Some(slice) = slices.next().await {
    let slice = slice?;
    // First stream: skip 5, length 5. Second stream: skip 0, length 15.
    // Pass these local plaintext ranges to stream-system.
}
```

`PartId` belongs to one blob. Saving an existing ID replaces its stream reference
and length while the blob is open. Different parts may reference the same stream. Finalization uses
exactly the supplied order, including repetitions, and excludes unselected parts.
An empty list produces a zero-length blob. Zero-length streams are allowed.

`can_save(blob_id)` returns `Ok(())` only when the blob exists and is open.
It reports `BlobNotFound` or `BlobFinalized` otherwise. This read-only transaction
lets the caller reject an upload before allocating backend files. It reserves
nothing: finalization may follow the check, so `save_part` repeats the same checks
in its insertion or replacement transaction. Concurrent saves of the same part
retain the last committed stream reference and length.

`get_parts(blob_id)` returns a `BTreeMap<PartId, BlobPart>` containing all stored
part references and lengths, sorted by part ID. It accepts open and finalized
blobs, including parts excluded from the manifest. An existing blob without
parts returns an empty map; a missing blob returns `BlobNotFound`. It reads no
stream content, reserves nothing, and provides no atomic snapshot across
concurrent writes.

Finalization checks every selected part and rejects total lengths exceeding
`u64`. Missing parts and length overflow leave an open blob unchanged. Sled
transactions coordinate `save_part` and `finalize` across registry clones. After
finalization, saving parts fails. Finalizing with the same list returns the
original length; a different list fails.

Every successful mutation awaits a durable `Db::flush` on a separate thread.
This avoids sled 0.34.7's shared-pool deadlock when flush workers wait for log
writes queued on that same pool, without blocking the async executor. The cost
is one thread per flush; thread creation and worker failures are returned.
A storage/flush error or a
cancelled future can occur after transaction commit; an error does not imply
rollback. Retrying finalization with the same list also flushes the database.
After an uncertain part save, a retry replaces the part if the blob is still open.
An uncertain create can leave an unused open blob.

`read(blob_id, start..end)` requires a finalized blob and an exclusive end with
`start <= end <= length`. Empty ranges return an empty stream, including at EOF.
The returned stream owns its plan, visits part metadata lazily, and stops after
its first error. Each item gives `stream_id`, a local `skip`, and the number of
bytes to read. A complete read uses `0..length`.

There is no delete operation. Open blobs and unselected parts remain stored.

## Optional gRPC

`BlobRegistry` always supports Local. `grpc-client` adds `RemoteRegistry` and the
Remote enum variant; `grpc-server` adds `GrpcService::new(registry).into_server()`.
The service delegates to the supplied enum, including Remote for relays.
`grpc-tls` extends enabled transports without enabling a client or server itself.
`cli` remains independent. No gRPC dependency or `protoc` is required for local
builds; gRPC builds require `protoc` in `PATH` or the `PROTOC` environment variable.

```sh
cargo run -p blob-registry --features cli,grpc-server -- \
  --listen 0.0.0.0:50052 --blob-registry-path ./data/blob-registry
```

The default address is `127.0.0.1:50052`. Ctrl-C waits for graceful shutdown.
With `grpc-client`, `--blob-registry-endpoint` or
`WD40_BLOB_REGISTRY_ENDPOINT` selects Remote without opening sled.
`--blob-registry-path` and `WD40_BLOB_REGISTRY_PATH` keep the local defaults.
`BlobRegistryConfig::open` is async; library clients use
`RemoteRegistry::connect(tonic::transport::Endpoint)` inside a Tokio runtime.
Clones share the channel without an application mutex.

TLS is optional on the secured private network. With `grpc-tls`, paired
`--tls-cert` and `--tls-key` PEM files enable incoming TLS. Clients use HTTPS with
verified certificates and hostnames; `--blob-registry-ca` or
`WD40_BLOB_REGISTRY_CA` adds a PEM CA to system trust roots. HTTPS is rejected
without the feature. There is no mTLS or additional authentication.

All IDs are 16 big-endian bytes, except native u64 version IDs. The protocol is
`wd40.blob_registry.v1`. Both encoding and decoding are limited to 4 MiB per
message. Structured status details preserve domain error variants and fields;
storage/backend and unrecognized errors remain `Error::Rpc`. Relays preserve
unknown statuses. Failed calls are not replayed and never fall back to Local.
Endpoints must address the same logical namespace and relay chains must not cycle.
Successful writes retain the durable-flush guarantee; an error or cancellation
does not imply rollback.

Create, CanSave, SavePart, GetParts and Finalize are unary RPCs. Read streams owned
`StreamSlice` plans while preserving ranges, order, repetitions and terminal
errors. An identical Finalize retains its existing idempotency. No stream content
is transferred or existence-checked.

Concurrency is a required acceptance gate:

```sh
cargo test -p blob-registry --all-features -- --test-threads=16
cargo test --workspace --all-features -- --test-threads=64
```

Tests cover Remote and relay parity, domain errors, malformed/oversized requests,
TLS verification, CLI selection, and 32 concurrent clients on a Tokio runtime
with 16 workers using independent databases and a shared server. Local flushes
retain their separate-thread implementation to avoid sled I/O-pool starvation.
