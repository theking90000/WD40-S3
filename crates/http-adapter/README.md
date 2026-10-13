# HTTP Adapter

Create anonymous objects and stream their contents through a cloneable Hyper
service. The adapter composes `ObjectRegistry` and `BlobSystem` directly. It does
not authenticate requests or contact the access controller.

| Request | Response |
| --- | --- |
| `POST /object` | `201 Created`, `Location: /object/<id>`, empty body |
| `GET /object/<id>` | `200 OK` or `206 Partial Content`, streaming body |
| `HEAD /object/<id>` | Full-object headers, empty body |

Each POST generates a 128-bit random ID encoded as 22 case-sensitive base62
characters (`0-9`, `A-Z`, `a-z`). Existing 32-character lowercase hexadecimal IDs
remain readable. The object registry key is `<prefix>/<id>`. The prefix is required,
cannot be empty or contain NUL, and has trailing slashes removed. Public URLs
contain only the ID. New objects are published after the blob is finalized and
the registry stores its reference, plaintext `size` and `content-type` metadata.
Uploads without a content type use `application/octet-stream`.

Send custom metadata as `X-Meta-<name>` headers. The registry stores their full
lowercase header names and string values; GET and HEAD return them as response
headers. Repeated values are joined with `, `. An empty metadata name or a
non-text value returns `400` before storage. For example, `X-Meta-Yo: true`
stores `x-meta-yo = "true"`. Other request headers are not stored as custom
metadata. Metadata headers cannot replace the object's size or content type.

## Embed in Hyper

```rust,no_run
use blob_system::{BlobSystem, BlobWriterConfig, FrameBudget, SchedulerConfig};
use http_adapter::{HttpAdapter, HttpAdapterConfig};
use object_registry::ObjectRegistry;

fn service(
    objects: ObjectRegistry,
    blobs: BlobSystem,
    writer: BlobWriterConfig,
    scheduler: SchedulerConfig,
    budget: FrameBudget,
) -> std::io::Result<HttpAdapter> {
    HttpAdapter::new(
        HttpAdapterConfig { prefix: "uploads".to_owned(), writer, scheduler },
        objects,
        blobs,
        budget,
    )
}
```

Pass this service to Hyper's `serve_connection`, or use `Service::call` from an
existing router. It accepts any Send HTTP body producing `Bytes` whose errors
implement `std::error::Error + Send + Sync`. The service future and `HttpBody`
are Send; the response body does not require Sync.

Construction validates prefix, writer and scheduler settings without I/O.
The application supplies the storage handles, budget, listener, runtime and
TLS. Each operation owns its scheduler configuration and uploads own their
allocator. Clones share the frame budget. CARBON is polled directly, without
dedicated tasks or a mutex around transfers. HTTP response frames own decoded
buffers without copying their payload; only the requested bytes are transmitted.
Memory stays bounded by scheduler windows, the shared budget and retained HTTP
frames, rather than object size.

## Byte ranges and errors

GET supports one range: `bytes=start-end`, `bytes=start-`, or `bytes=-suffix`.
End positions in the HTTP header are inclusive. Partial responses include
`Content-Range` and `Content-Length`. Unsatisfiable ranges return `416` with
`Content-Range: bytes */<size>`. Malformed ranges return `400`.

Multiple ranges, multiple Range fields and unknown range units produce a full
`200` response. HEAD ignores Range, returns the full object's length and reads
no backend content. Responses advertise `Accept-Ranges: bytes`. This version
has no ETag validators; If-Range requests receive the full object.

Missing objects/routes return `404`, malformed IDs return `400`, and unsupported
methods return `405` with `Allow`. Storage failures are logged on stderr and return
`500` without exposing internal error details in the response. The first GET frame is read before headers are returned;
a later read failure interrupts the body. HEAD responses always have no body,
including errors. Empty uploads are supported without opening a backend file.

An interrupted upload does not publish an object key. Already-created storage
records are not rolled back. A registry error or cancellation can occur after a
commit; retrying POST creates a new ID and is not a recovery operation.

## Standalone

```sh
cargo run -p http-adapter --features standalone -- \
  --prefix uploads --listen 127.0.0.1:8080 \
  --webhook 'https://discord.com/api/webhooks/<id>/<token>'
```

`cli` exposes `cli::HttpAdapterArgs` for `#[command(flatten)]`, including `--prefix`
or `WD40_HTTP_PREFIX` and the existing blob codec/CARBON options. `standalone`
enables the optional HTTP/1.1 executable, registry/backend CLI configurations,
pool construction and graceful Ctrl-C shutdown. The default listener is
`127.0.0.1:8080`. Incoming TLS belongs to the embedding application or its proxy.

Registries default to their existing `./data/*-registry` locations.
`grpc-client` enables their remote endpoint options; `grpc-tls` enables their
existing verified HTTPS transport. These features do not enable the executable
on their own. Backend environment values remain hidden in CLI help.

```sh
curl -i --data-binary @file.bin -H 'Content-Type: application/octet-stream' \
  -H 'X-Meta-Yo: true' \
  http://127.0.0.1:8080/object
curl -I http://127.0.0.1:8080/object/<id>
curl -H 'Range: bytes=0-1023' http://127.0.0.1:8080/object/<id>
```

## Checks

```sh
cargo test -p http-adapter --all-features
cargo check -p http-adapter --no-default-features
cargo check -p http-adapter --features cli
cargo check -p http-adapter --features standalone
cargo check -p http-adapter --features grpc-client,grpc-tls
cargo run -p http-adapter --features standalone -- --help
```

Tests cover ranges, routes, metadata, empty POST/HEAD/GET and Send contracts.
The deterministic loopback smoke test runs four concurrent nonempty uploads
through the real Discord backend implementation, local registries, AES-256 and
LZ4, then checks full reads, ranges and interrupted inputs. Its server simulates
Discord HTTP; it does not contact Discord or use external credentials.
