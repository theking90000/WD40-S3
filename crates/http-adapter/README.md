# HTTP adapter

Upload anonymous objects and stream their contents over HTTP. `http-adapter`
connects `ObjectRegistry` and `BlobSystem` through a cloneable Hyper service.
Requests are unauthenticated.

| Request | Result |
| --- | --- |
| `POST /object` | Creates an object and returns `201 Created` with its URL in `Location` and an empty body. |
| `GET /object/<id>` | Streams the object with `200 OK`, or a byte range with `206 Partial Content`. |
| `HEAD /object/<id>` | Returns the full object's headers with an empty body. |

## Run the server

```sh
cargo run -p http-adapter --features standalone -- \
  --prefix uploads --listen 127.0.0.1:8080 \
  --webhook 'https://discord.com/api/webhooks/<id>/<token>'
```

The standalone server uses HTTP/1.1 and shuts down gracefully on Ctrl-C. Its
default listener is `127.0.0.1:8080`; incoming TLS belongs to your application or
proxy. Registries default to their `./data/*-registry` locations.

Upload a file, then use the URL returned in `Location` to inspect or read it:

```sh
curl -i --data-binary @file.bin \
  -H 'Content-Type: application/octet-stream' \
  -H 'X-Meta-Yo: true' \
  http://127.0.0.1:8080/object

curl -I 'http://127.0.0.1:8080/object/<id>'
curl -H 'Range: bytes=0-1023' 'http://127.0.0.1:8080/object/<id>'
```

`--prefix` is required and also accepts `WD40_HTTP_PREFIX`. It namespaces registry
keys as `<prefix>/<id>` without appearing in public URLs. It must be nonempty
and contain no NUL characters; trailing slashes are removed.

| Feature | Use |
| --- | --- |
| `cli` | Embed `cli::HttpAdapterArgs` with `#[command(flatten)]`, including prefix, blob codec and CARBON options. |
| `standalone` | Run the HTTP executable with registry/backend CLI settings and pool construction. |
| `grpc-client` | Configure remote registry endpoints. |
| `grpc-tls` | Enable verified HTTPS transport for the registries. |

Enable `standalone` explicitly when running the executable with the gRPC features.

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

Pass the service to Hyper's `serve_connection`, or call it through `Service::call`
in an existing router. Your application supplies the storage handles, listener,
runtime and TLS. Construction validates the prefix, writer and scheduler settings
without I/O.

Request bodies must be `Send`, produce `Bytes`, and have errors implementing
`std::error::Error + Send + Sync`. The service future and response body are `Send`;
the response body need not be `Sync`.

Transfers stream data with memory bounded by scheduler windows, the shared frame
budget and retained HTTP frames. Service clones share the frame budget.

## Uploads and metadata

Each upload receives a random 128-bit ID encoded as 22 case-sensitive base62
characters using `0-9`, `A-Z` and `a-z`. Existing 32-character lowercase hexadecimal
IDs remain readable. Empty uploads are supported.

The adapter publishes an object after finalizing its blob and storing the blob
reference, plaintext size and content type in the registry. If the upload omits
`Content-Type`, the adapter uses `application/octet-stream`.

Use `X-Meta-<name>` headers for custom metadata. GET and HEAD return these headers
with lowercase names; repeated values are joined with `, `. For example,
`X-Meta-Yo: true` is stored as `x-meta-yo = "true"`. Empty names and non-text values
return `400` before storage. Other request headers are not stored as custom
metadata, and metadata cannot override the object's size or content type.

Interrupted uploads do not publish an object key, but already-created storage
records are not rolled back. A registry error or cancellation can occur after a
commit. Retrying POST creates a new ID; it does not recover the previous upload.

## Ranges and errors

GET accepts a single byte range:

| Header | Selection |
| --- | --- |
| `Range: bytes=start-end` | Bytes from `start` through `end`, inclusive. |
| `Range: bytes=start-` | Bytes from `start` to the end of the object. |
| `Range: bytes=-suffix` | The last `suffix` bytes. |

Partial responses include `Content-Range` and `Content-Length`. Unsatisfiable
ranges return `416` with `Content-Range: bytes */<size>`; malformed ranges return
`400`. Multiple ranges, multiple `Range` fields and unknown range units return
the full object with `200 OK`.

Responses advertise `Accept-Ranges: bytes`. HEAD ignores ranges and returns the
full object's length without reading backend content. There are no ETag
validators; requests with `If-Range` receive the full object.

| Error | Response |
| --- | --- |
| Missing object or route | `404` |
| Malformed ID | `400` |
| Unsupported method | `405` with `Allow` |
| Storage failure before the response starts | `500` with internal details logged to stderr only |

GET reads its first frame before returning headers. A later read failure
interrupts the response body. HEAD responses have no body, including errors.
