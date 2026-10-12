# stream-registry

Stores immutable stream parts and ordered manifests, including the CRAFT metadata
needed to reconstruct content. It transfers no file bytes and does not check the
referenced file registry.

`StreamRegistry` selects `Local` or, with `grpc-client`, `Remote`. Both implement
the same trait. The gRPC server accepts this enum and delegates without inspecting
the variant, so it can also relay calls to another server.

## Features

| Feature | Enables |
| --- | --- |
| None | Local sled registry |
| `grpc-client` | `RemoteRegistry` and the `Remote` enum variant |
| `grpc-server` | `GrpcService` and the generated Tonic server |
| `grpc-tls` | Optional TLS with certificate and hostname verification |
| `cli` | Composable Clap configuration |

gRPC builds require `protoc` in `PATH`, or an executable specified by `PROTOC`.
Local builds do not invoke it or compile the gRPC build dependencies. TLS alone
does not activate a client or server. Remote calls require a Tokio runtime.

## Standalone server

```sh
cargo run -p stream-registry --features cli,grpc-server -- \
  --listen 0.0.0.0:50051 --stream-registry-path ./data/stream-registry
```

The default listen address is `127.0.0.1:50051`. Ctrl-C shuts the server down
gracefully. To run a relay, enable `grpc-client` and supply
`--stream-registry-endpoint http://upstream:50051` instead of a local database.

`StreamRegistryConfig::open().await` selects Remote when the endpoint is present
and does not open sled. Otherwise it opens the configured local path. The matching
environment variables are `WD40_STREAM_REGISTRY_ENDPOINT` and
`WD40_STREAM_REGISTRY_PATH`.

For an existing `stream-system` CLI, enable the dependency feature explicitly:

```sh
cargo run -p stream-system --features cli,stream-registry/grpc-client -- \
  --stream-registry-endpoint http://registry:50051 download 123
```

## Embedded client and server

```rust,no_run
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
use stream_registry::{RemoteRegistry, StreamRegistry, traits::StreamRegistry as _};
use tonic::transport::Endpoint;

let remote = RemoteRegistry::connect(Endpoint::from_static("http://registry:50051")).await?;
let registry = StreamRegistry::Remote(remote);
let empty_stream = registry.store(Vec::new()).await?;
# let _ = empty_stream;
# Ok(())
# }
```

With `grpc-server`, pass `GrpcService::new(registry).into_server()` to Tonic's
`Server::builder().add_service(...)`. This wrapper applies the same 4 MiB message
limits as the remote client. Cloned clients share the channel without an
application mutex.

## TLS and protocol guarantees

On the private secured network, plaintext HTTP/2 is the default. With `grpc-tls`,
the server accepts paired `--tls-cert` and `--tls-key` PEM files. Clients use HTTPS
and system trust roots; `--stream-registry-ca` or `WD40_STREAM_REGISTRY_CA` adds a
PEM CA. Library callers can configure TLS on the `Endpoint` passed to `connect`.
HTTPS is rejected without `grpc-tls`. There is no mTLS or additional authentication.

The `wd40.stream_registry.v1` Protobuf service has `Insert`, `Store`, and
server-streaming `RetrieveParts`. IDs are exactly 16 big-endian bytes; only CRAFT
metadata uses Postcard. The CRAFT 0.2.2 serialized layout is part of the v1 wire
contract. Changes to that layout require a compatible migration or a new protocol
version. CRAFT metadata can include content encryption keys; do not log payloads.

Ordering, repetitions, empty streams and lazy terminal read errors match the local
registry. Domain errors retain their variants and IDs through structured gRPC
status details; unrecognized errors remain `Error::Rpc`. Successful writes retain
the local durable-flush guarantee. Failures can occur after a write took effect.

The architecture assumes one namespace and 4 MiB messages. All endpoints and
relays must address the same logical storage. A response lost
after a write does not imply rollback. Connections may reconnect for subsequent
calls, but the application does not replay failed operations. Relays must not form
cycles. Byte transfers remain in the file backends.

## Extending other registries

Reuse the enum/client/server pattern per crate without adding a common framework.
Blob operations retain their atomic validations and finalization semantics;
object pages carry opaque cursors. File creation must receive the entire response
before returning its infallible item stream. No other registry is changed here.

## Checks

```sh
cargo test -p stream-registry
cargo test -p stream-registry --features grpc-client,grpc-server,cli
cargo test -p stream-registry --all-features
```

Integration tests use ephemeral servers and static TLS fixtures. The included
server key is exclusively a test key, never a deployment credential.
