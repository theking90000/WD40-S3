# Registry backends

`RegistryBackend` selects the backend at runtime. Its variants are enabled by the
`discord` and `telegram` features. One implementation of the
`backends::traits::RegistryBackend` trait delegates to the selected backend.

`AccessPath`, `WriteAccessPath` and `ReadAccessPath` are enums with a variant for
each enabled backend. All three derive `Serialize` and `Deserialize` and own
their data. The shared `Error` type in `error.rs` wraps connection and backend
errors while preserving their causes. The trait has no associated error type.

Telegram variants use `unimplemented!` until their implementation is added.
These explicitly requested placeholders are the only permitted panic points.

## Discord

`DiscordRegistry` owns a `tokio_tcp_pool::Pool` and two credential lists stored
in `RotatingVec<String>`. Webhook URLs and bot tokens are owned strings.

`DiscordRegistry::new` takes an existing pool and both credential lists. The
caller must configure the pool for TLS connections to `discord.com:443`.

Clones of a `RotatingVec` share its vector and atomic cursor. `get()` returns
successive elements in rotation and returns `None` when empty. Each credential
list has its own cursor. Rotation uses checked arithmetic and fallible access.

`DiscordAccessPath` wraps a stored `DiscordFile`. Its `is_expired()` method checks
the download URL expiration against the local clock.

`write(count)` selects webhook credentials in rotation. An empty webhook list
produces an empty stream. `DiscordWriteAccessPath` contains only the owned webhook
URL and derives `Serialize` and `Deserialize`. Upload settings are fixed to
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
