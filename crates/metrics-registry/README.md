# metrics-registry

Record gauge assignments and counter increments in atomic batches, then query
current gauges or historical windows. Local storage uses RocksDB. Optional gRPC
transports expose the same API through a remote client or a relay.

## API and local storage

`traits::MetricsRegistry` defines `record(Vec<Record<Metric>>)` and
`query(Vec<Query<Metric>>)`. Batches are consumed. Metric types only require
`Into<u16>`; they do not need `Copy`, `Clone`, `Send` or `Sync`. Identifiers are
converted before returning the operation's future. All `u16` IDs are supported,
including zero. Applications must keep their assignments stable and distinct.
The built-in `Metric` enum supplies stored bytes, downloaded bytes, uploaded
bytes and request count with IDs 1 through 4.

```rust
use metrics_registry::{
    MetricKind, MetricQuery, MetricRecord, MetricsRegistry, Query, Record,
    Selection, traits::MetricsRegistry as _,
};

let records = vec![Record {
    key: "/objects/photo".to_owned(),
    aggregates: vec!["/objects/".to_owned()],
    metrics: vec![MetricRecord {
        metric: 42_u16,
        kind: MetricKind::Gauge,
        value: 150,
    }],
}];
registry.record(records).await?;
let results = registry.query(vec![Query {
    prefixes: vec!["/objects/".to_owned()],
    metrics: vec![MetricQuery {
        metric: 42_u16,
        kind: MetricKind::Gauge,
        selection: Selection::Current,
    }],
}]).await?;
```

`MetricsRegistry::Local` and `MetricsRegistry::Remote` delegate the same trait.
Construct local storage with `LocalRegistry::new(database, config)`. Open its
RocksDB column families with `LocalRegistry::column_families(&options)` to install
the required native merge operators. The default feature set is empty; local
operations run blocking storage work on a worker pool and need no Tokio runtime.

Gauge values are absolute assignments; counter values are nonnegative increments.
The backend assigns record timestamps. Each record batch is validated before
mutations and success awaits durable persistence. A backend or transport error
does not guarantee rollback. Query batches preserve query and metric order and
read one consistent view. Prefix unions do not double-count overlapping keys.
An empty prefix list selects nothing; an empty string selects the whole namespace.

History selections carry a resolution and an absolute start-inclusive,
end-exclusive range. Resolutions must match the configured policies. Windows
are aligned to the Unix epoch; `Point::complete` is false for the open window.
Counters have no resolution-independent current selection. Configuration is not
persisted and must be supplied again when opening the database. Historical
retention cleanup is not implemented yet; current gauge values do not expire.

## Features

| Feature | Enables |
| --- | --- |
| `cli` | Composable `cli::MetricsRegistryConfig` and its `open()` method |
| `grpc-client` | `RemoteRegistry` and `MetricsRegistry::Remote` |
| `grpc-server` | `GrpcService::new(registry).into_server()` |
| `grpc-tls` | TLS for the enabled gRPC transports |

The server binary requires `cli,grpc-server`. gRPC builds require `protoc` in
`PATH` or selected through `PROTOC`. Local-only builds do not generate Protobuf
code or enable tonic dependencies.

## Local server

```sh
cargo run -p metrics-registry --features cli,grpc-server -- \
  --listen 127.0.0.1:50057 \
  --metrics-registry-path ./data/metrics-registry
```

The default listen address is `127.0.0.1:50057`; the default database directory
is `./data/metrics-registry`. Ctrl-C requests graceful shutdown.

When neither `--metrics-registry-policy` nor `WD40_METRICS_REGISTRY_POLICY` is
supplied, the CLI uses these local history policies:

| Resolution | Retention | Points per retention period | CLI value in seconds |
| --- | --- | --- | --- |
| 5 minutes | 6 hours | 72 | `300:21600` |
| 30 minutes | 48 hours | 96 | `1800:172800` |
| 1 hour | 14 days | 336 | `3600:1209600` |
| 1 day | 1 year, 365 days | 365 | `86400:31536000` |

Custom policies replace the defaults. Use
`--metrics-registry-policy RESOLUTION:RETENTION`, with both numbers in whole
seconds. Resolutions must be nonzero and distinct; each retention must be at
least as long as its resolution. These defaults apply to
`MetricsRegistryConfig`, including when used by the S3 adapter or embedded S3
in the portal. Direct `LocalRegistry` callers supply their own `Config`.

| Option | Environment variable |
| --- | --- |
| `--metrics-registry-path` | `WD40_METRICS_REGISTRY_PATH` |
| `--metrics-registry-policy` | `WD40_METRICS_REGISTRY_POLICY` |
| `--metrics-registry-endpoint` | `WD40_METRICS_REGISTRY_ENDPOINT` |
| `--metrics-registry-ca` | `WD40_METRICS_REGISTRY_CA` |

Policies may be repeated or separated by commas. For example,
`WD40_METRICS_REGISTRY_POLICY=300:86400,3600:2592000` configures both policies.
`MetricsRegistryConfig` can be flattened into another Clap command.

## Remote clients and relays

With `grpc-client`, `--metrics-registry-endpoint` selects the remote registry
without opening a local database or requiring local retention policies.
The receiving backend owns timestamps, retention and storage.

```rust
use metrics_registry::{MetricsRegistry, RemoteRegistry};
use tonic::transport::Endpoint;

let remote = RemoteRegistry::connect(
    Endpoint::from_static("http://127.0.0.1:50057"),
).await?;
let registry = MetricsRegistry::Remote(remote);
```

Connections and remote operations require a Tokio runtime. Clones share their
HTTP/2 channel, and each operation clones the generated client without an
application mutex. Calls are not automatically replayed, split or redirected.
There is no fallback to local storage. An ambiguous recording failure must not
be blindly retried, because counter increments may already have been persisted.

A server can forward both RPCs through an injected remote registry:

```sh
cargo run -p metrics-registry --features cli,grpc-client,grpc-server -- \
  --listen 127.0.0.1:50058 \
  --metrics-registry-endpoint http://127.0.0.1:50057
```

## TLS

Add `grpc-tls` and supply paired `--tls-cert` and `--tls-key` PEM files to enable
incoming TLS. Outgoing HTTPS connections configured through the CLI verify
certificates and hostnames with system trust roots. `--metrics-registry-ca`
adds a PEM CA and requires an HTTPS endpoint. HTTPS requires `grpc-tls`.
When connecting programmatically, supply the desired
`tonic::transport::ClientTlsConfig` on the `Endpoint`.

## Protocol and errors

`wd40.metrics_registry.v1` exposes unary `Record` and `Query` RPCs. Each complete
batch is one RPC and one backend operation. Protobuf messages carry typed records,
queries, selections and results. Metric IDs must fit `u16`; unknown metric-kind
enum values, missing required fields and invalid time encodings are rejected.
Durations use unsigned seconds plus nanoseconds. Timestamps use a sign and a
duration relative to the Unix epoch, with checked conversion to `SystemTime`.
The transport preserves nanosecond precision; the local backend still requires
whole-second history resolutions and rejects query times before the Unix epoch.

Both transports enforce a 4 MiB encoding and decoding limit. Oversized batches
are rejected rather than split. Clients check response counts, selection types,
required point fields and increasing historical timestamps before returning
results.

Structured status details preserve `InvalidRecord`, `InvalidQuery`,
`ResolutionUnavailable`, `HistoryExpired` and `Overflow`, including across relays.
`InvalidRecord` and `InvalidQuery` reasons use `Cow<'static, str>` to support owned
remote messages. Storage failures return a generic internal status. Other RPC
failures remain `Error::Rpc`; connection failures are `Error::Transport`.
