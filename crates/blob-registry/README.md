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
registry.can_save(blob_id, 1).await?; // Check before allocating/uploading a stream.
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

`PartId` belongs to one blob. Saving an existing ID fails even when its stream and
length match. Different parts may reference the same stream. Finalization uses
exactly the supplied order, including repetitions, and excludes unselected parts.
An empty list produces a zero-length blob. Zero-length streams are allowed.

`can_save(blob_id, part_id)` returns `Ok(())` only when the blob exists, is open
and the part ID is absent. It reports `BlobNotFound`, `BlobFinalized` or
`PartAlreadyExists` otherwise. This read-only transaction lets the caller reject
an upload before allocating backend files. It reserves nothing: another save or
finalization may follow the check, so `save_part` repeats the same checks in its
insertion transaction.

Finalization checks every selected part and rejects total lengths exceeding
`u64`. Missing parts and length overflow leave an open blob unchanged. Sled
transactions coordinate `save_part` and `finalize` across registry clones. After
finalization, saving parts fails. Finalizing with the same list returns the
original length; a different list fails.

Every successful mutation awaits `flush_async`. A storage/flush error or a
cancelled future can occur after transaction commit; an error does not imply
rollback. Retrying finalization with the same list also flushes the database.
After an uncertain part save, a retry may report `PartAlreadyExists`. An uncertain
create can leave an unused open blob.

`read(blob_id, start..end)` requires a finalized blob and an exclusive end with
`start <= end <= length`. Empty ranges return an empty stream, including at EOF.
The returned stream owns its plan, visits part metadata lazily, and stops after
its first error. Each item gives `stream_id`, a local `skip`, and the number of
bytes to read. A complete read uses `0..length`.

There is no delete operation. Open blobs and unselected parts remain stored.
