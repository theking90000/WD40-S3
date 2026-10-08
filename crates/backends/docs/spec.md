# Registry backends

`RegistryBackend` selects the backend at runtime. Its variants are enabled by the
`discord` and `telegram` features. One implementation of the
`backends::traits::RegistryBackend` trait delegates to the selected backend.

`AccessPath`, `WriteAccessPath` and `ReadAccessPath` are enums with a variant for
each enabled backend. All three derive `Serialize` and `Deserialize` for
persistence. Write paths own shared webhook URLs. The shared
`Error` type in `error.rs` wraps connection and backend
errors while preserving their causes. The trait has no associated error type.

Telegram variants use `unimplemented!` until their implementation is added.
These explicitly requested placeholders are the only permitted panic points.

## Discord

`DiscordRegistry` owns a `tokio_tcp_pool::Pool` and two credential lists.
Webhooks are stored in `RotatingVec<DiscordWriteAccessPath>`;
bot tokens remain owned strings in `RotatingVec<String>`.

`DiscordRegistry::new` takes an existing pool, webhook URLs and bot tokens.
Webhook URLs are owned and shared through `Arc<str>` without copying their
contents for each generated write path.
The caller must configure the pool for TLS connections to
`discord.com:443`.

Clones of a `RotatingVec` share its vector and atomic cursor. `get()` returns
successive elements in rotation and returns `None` when empty. Each credential
list has its own cursor. Rotation uses checked arithmetic and fallible access.

`DiscordAccessPath` wraps a stored `DiscordFile`. Its `is_expired()` method checks
the download URL expiration against the local clock.

`write(count)` selects webhook credentials in rotation. An empty webhook list
produces an empty stream. The existing `DiscordWriteAccessPath` owns an `Arc<str>`.
Cloning a write path shares the URL allocation. Backend traits and enums have
no credential lifetime parameters. The published dependency exposes borrowed
credentials with private fields, so the URL is parsed locally when opening an
upload. Temporary paths require no serialization or URL reconstruction.
Upload settings are fixed to
`discord_webhook_storage::WriteConfig::default()` and need no stored configuration.
Generating a write path does not open an upload.

`read(access_paths)` returns valid stored URLs directly. Expired URLs are renewed
individually using bot credentials selected in rotation and a connection acquired
from the supplied pool. Renewed URLs are returned without modifying the stored
access paths. Connections are released after successful renewal and discarded
when renewal fails or is interrupted. Requests are sequential with no retries.

Streams from the backend enum wrap their results in the corresponding path
variant. An access path belonging to another backend returns `BackendMismatch`.
Read errors preserve missing credentials, connection failures and renewal errors.

To add a backend, add its variants to the backend and path enums, then add the
matching `write`, `read` and expiration dispatch arms. Each backend implements
only `write` and `read`. Boxed streams unify the concrete stream types in the
enum dispatch without additional methods.
