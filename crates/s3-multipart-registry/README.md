# S3 multipart registry

Stores immutable multipart upload sessions using sled and Postcard. Implements
`traits::S3MultipartRegistry` through `LocalRegistry`, `RemoteRegistry` and the
`S3MultipartRegistry::{Local, Remote}` enum.

`create(upload)` generates a cryptographically random 128-bit identifier,
encoded as 32 lowercase hexadecimal ASCII characters. `MultipartUploadId`
validates this format during construction, parsing and deserialization.
Generated ID collisions are retried without overwriting existing sessions.

Each session contains `blob_id`, `bucket`, `key` and a `BTreeMap<String, String>`
of metadata. Store the content type under `content-type` in that map. Values
are preserved without interpretation or checks against a blob registry.

Two sled trees hold the session and its reverse blob mapping. Transactions
ensure that each registered blob has exactly one session. Duplicate blobs
return `Error::BlobAlreadyExists(blob_id)`. `get(id)` returns `None` when absent.
`remove(id)` atomically removes both mappings, succeeds when absent and never
removes the blob. The blob can be registered again after removal.

Successful mutations await a durable flush using the other registries' named
thread and oneshot pattern. A flush or transport failure does not imply
rollback. Creating a blob and registering its session are separate operations;
an error can leave an unregistered blob behind. Remote `create` calls are not
retried automatically after an uncertain result.

```rust
use std::collections::BTreeMap;
use s3_multipart_registry::{
    LocalRegistry, MultipartUpload, traits::S3MultipartRegistry as _,
};

async fn example() -> Result<(), s3_multipart_registry::Error> {
    let registry = LocalRegistry::new(sled::open("./data/s3-multipart-registry")?)?;
    let id = registry.create(MultipartUpload {
        blob_id: 42,
        bucket: "bucket".to_owned(),
        key: "file.txt".to_owned(),
        metadata: BTreeMap::from([("content-type".to_owned(), "text/plain".to_owned())]),
    }).await?;
    let _session = registry.get(&id).await?;
    registry.remove(&id).await?;
    Ok(())
}
```

Features default to local-only, without tonic or protoc. `cli` exposes
`S3MultipartRegistryConfig` for `#[command(flatten)]`. `grpc-client` enables
`RemoteRegistry::connect(Endpoint)`; `grpc-server` enables `GrpcService` and,
with `cli`, the standalone server. `grpc-tls` enables optional TLS. gRPC builds
require protoc. The wire protocol uses typed protobuf messages and 16-byte
big-endian blob IDs. Both directions limit messages to 4 MiB.

```sh
cargo run -p s3-multipart-registry --features cli,grpc-server -- \
  --listen 127.0.0.1:50056 \
  --s3-multipart-registry-path ./data/s3-multipart-registry
```

The defaults are `127.0.0.1:50056` and `./data/s3-multipart-registry`. Ctrl-C
requests graceful shutdown. CLI options use the `s3-multipart-registry` prefix
and environment variables `WD40_S3_MULTIPART_REGISTRY_PATH`,
`WD40_S3_MULTIPART_REGISTRY_ENDPOINT` and `WD40_S3_MULTIPART_REGISTRY_CA`.
Selecting an endpoint connects remotely without opening the local database,
including when serving a relay. The optional CA requires HTTPS. Server TLS
uses `--tls-cert` and `--tls-key`. Clients share a channel without application
retries, a mutex or local fallback.

This crate stores sessions. S3 authorization, request bucket/key matching,
blob creation, parts, completion and abort cleanup belong to the caller.
There is no principal field, listing, expiration or lifecycle state machine.
