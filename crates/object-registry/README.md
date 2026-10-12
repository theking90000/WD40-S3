# object-registry

Store versioned object keys, blob references and opaque metadata in sled.
The registry stores no content, does not check or delete blobs, and interprets
none of the metadata fields. Empty maps and arbitrary string values are valid;
size and ETag handling belong to callers.

Like the other registries, the crate provides `traits::ObjectRegistry`,
`LocalRegistry` and the delegating `ObjectRegistry::Local` enum. Construct a
local registry with a caller-owned `sled::Db`; clones and independently
constructed handles over the same database share its state. The crate opens
only `object-active` and `object-versions` and uses postcard for records.

```rust
use object_registry::{LocalRegistry, Metadata, traits::ObjectRegistry as _};

let registry = LocalRegistry::new(database)?;
let version = registry.store("photos/cat", blob_id, Metadata::new()).await?;
let record = registry.head("photos/cat", None).await?;
let historical = registry.head("photos/cat", Some(version)).await?;
let page = registry.list("photos/", None, Some(100), false).await?;
let marker = registry.delete("photos/cat", None).await?;
registry.delete("photos/cat", Some(marker.version_id)).await?;
```

## Records and versions

`head` and `list` return the same `RecordMetadata`: key, `VersionId`, optional
`BlobId`, complete metadata map and `is_delete_marker`. Keys are nonempty UTF-8
strings without NUL; prefixes may be empty but cannot contain NUL. There is no
metadata key restriction, timestamp, size validation or ETag generation.

`VersionId` is a sled-generated `u64`, unique within its database, including
after reopening. Generate it inside each transaction attempt after reading
the current pointer. Concurrent changes to the same key cause retries, which
can consume IDs; gaps are normal. IDs have no meaning outside their database.
The caller supplies a blob reference without any existence/finalization check.

`object-active` maps each visible key to its complete current record, so a
current `head` uses one lookup. Version records use
`0x01 || Key || 0x00 || BE(u64::MAX - VersionId)` in `object-versions`.
The same tree stores per-key current pointers at `0x00 || Key`, in a separate
namespace excluded from listings. Version records retain their predecessor,
deleted flag and object or marker content; key strings are not duplicated in
the stored values.

## Deletion

`delete(key, None)` always creates a new Delete Marker and removes the active
entry, including for absent/already-hidden objects. Current `head` then returns
`ObjectNotFound`; explicit historical object versions remain readable.
An explicitly requested marker has `blob_id = None`, an empty metadata map
and `is_delete_marker = true`.

`delete(key, Some(id))` sets an internal flag without removing the version's
record, metadata or blob reference. The flagged version becomes inaccessible
to `head` (`VersionNotFound`) and disappears from history listings. Missing or
already-flagged versions succeed idempotently and return the requested ID.
No extra marker is created by explicit deletion.

Deleting the current version follows predecessor links, skips flagged versions
and restores the newest remaining object or marker. A surviving marker keeps
the object absent; deleting that marker can reveal an older object. Deleting
an older version cannot change the current object. With no remaining version,
both the active entry and current pointer disappear. Stored version records
remain retained, even when every version has been deleted.

All mutations use a transaction over the two trees. Predecessor traversal uses
transactional `get` calls; there is no scan inside a transaction or application
mutex. Links must point to strictly smaller IDs. Invalid links, missing linked
records and serialization errors abort the mutation without partial changes.
Reads perform single-tree lookups/scans without read transactions.

## Pagination

`list(prefix, cursor, limit, false)` scans only current objects.
With `include_versions = true`, it scans historical records, including
non-deleted markers and excluding flagged versions. Both modes return complete
metadata. Ordering is lexicographic by UTF-8 key bytes, then descending ID for
each key's history, not chronological across different keys.

The default limit is 1,000; zero is rejected. `next_cursor` is provided only
when another visible record exists. Resume using the same prefix and mode:

```rust
if let Some(cursor) = page.next_cursor {
    let next = registry.list("photos/", Some(&cursor), Some(100), false).await?;
}
```

`Cursor` has private fields and supports Serde serialization, including
postcard. It retains the prefix, mode and exclusive last-returned storage key;
it does not require that record to still exist or remain visible. Incompatible
or malformed cursors are rejected. Pages are not snapshots: concurrent changes
may appear in later pages or behind a cursor. Historical scans may need to skip
many retained deleted records; current listings never scan those records.

## Durability and errors

Every successful store or delete awaits a durable flush, including idempotent
deletes. Sled 0.34.7's `flush_async` can exhaust its shared I/O pool when many
databases flush concurrently: its flush workers wait for log writes queued on
the same pool. This registry runs `Db::flush` on a separate thread and awaits
its result through a oneshot channel. It requires no async runtime or added
application mutex; the cost is one thread per flush.

A storage/flush error or future cancellation can happen after the
transaction committed; an error does not imply rollback. Retrying a store or
simple delete creates another version. Retrying explicit deletion is idempotent.

Errors preserve sled/postcard causes and distinguish invalid input, missing
objects/versions and corrupt internal state. The crate has no CLI, HTTP adapter,
blob deletion, reference counting or garbage collection.

Integration tests cover version/marker lifecycle, opaque metadata, pagination,
independent concurrent handles, database reopening and corruption rollback.
A bounded regression test also flushes sixteen independent databases at once.
Run them separately when validation is requested:

```sh
cargo test -p object-registry
```
