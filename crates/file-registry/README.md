# file-registry

Allocates file references and resolves provider access paths. Content I/O belongs
to FileBackend. Local uses sled for pending/available states and an expiring cache.

## Optional gRPC

`FileRegistry` always supports Local. `grpc-client` adds `RemoteRegistry` and the
Remote enum variant; `grpc-server` adds `GrpcService::new(registry).into_server()`.
The service delegates to the supplied enum, including Remote for relays.
`grpc-tls` extends enabled transports without enabling a client or server itself.
`cli` remains independent. No gRPC dependency or `protoc` is required for local
builds; gRPC builds require `protoc` in `PATH` or the `PROTOC` environment variable.

```sh
cargo run -p file-registry --features cli,grpc-server -- \
  --listen 0.0.0.0:50054 --file-registry-path ./data/file-registry
```

The default address is `127.0.0.1:50054`. Ctrl-C waits for graceful shutdown.
With `grpc-client`, `--file-registry-endpoint` or
`WD40_FILE_REGISTRY_ENDPOINT` selects Remote without opening sled.
`--file-registry-path` and `WD40_FILE_REGISTRY_PATH` keep the local defaults.
`FileRegistryConfig::open` is async; library clients use
`RemoteRegistry::connect(tonic::transport::Endpoint)` inside a Tokio runtime.
Clones share the channel without an application mutex.

TLS is optional on the secured private network. With `grpc-tls`, paired
`--tls-cert` and `--tls-key` PEM files enable incoming TLS. Clients use HTTPS with
verified certificates and hostnames; `--file-registry-ca` or
`WD40_FILE_REGISTRY_CA` adds a PEM CA to system trust roots. HTTPS is rejected
without the feature. There is no mTLS or additional authentication.

All IDs are 16 big-endian bytes, except native u64 version IDs. The protocol is
`wd40.file_registry.v1`. Both encoding and decoding are limited to 4 MiB per
message. Structured status details preserve domain error variants and fields;
storage/backend and unrecognized errors remain `Error::Rpc`. Relays preserve
unknown statuses. Failed calls are not replayed and never fall back to Local.
Endpoints must address the same logical namespace and relay chains must not cycle.
Successful writes retain the durable-flush guarantee; an error or cancellation
does not imply rollback.

Create and Store are unary RPCs; Retrieve streams paths in request order,
including repeated IDs, and stops after the first error. Create receives and
validates the entire response before exposing its infallible item stream. Count
is bounded by the 4 MiB response ID budget; actual serialized paths must also fit
the message limit. Backend path enums use the v1 Postcard layout, with Discord as
its first variant; endpoints must use compatible backend layouts. Paths can
contain credentials and should not be logged. No file bytes cross gRPC.

The standalone Local server uses Discord registry options `--webhook` and
`--bot-token`, plus their existing `WD40_DISCORD_*` variables. Its HTTPS/HTTP1 pools
are configured for Discord URL renewal independently of optional gRPC TLS.
`FileRegistryConfig::open(backend).await` keeps the caller-owned backend API; a
Remote registry ignores that backend. The `stream-system` CLI can select it with
`--features cli,file-registry/grpc-client` and `--file-registry-endpoint`.

Concurrency is a required acceptance gate:

```sh
cargo test -p file-registry --all-features -- --test-threads=16
cargo test --workspace --all-features -- --test-threads=64
```

Tests cover Remote and relay parity, domain errors, malformed/oversized requests,
TLS verification, CLI selection, and 32 concurrent clients on a Tokio runtime
with 16 workers using independent databases and a shared server. Local flushes
retain their separate-thread implementation to avoid sled I/O-pool starvation.
