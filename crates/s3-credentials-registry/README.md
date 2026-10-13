# S3 credentials registry

Stores key IDs, secrets and principal IDs using sled and Postcard. The public
`traits::S3CredentialRegistry` trait is implemented by `LocalRegistry`,
`RemoteRegistry` and the `S3CredentialRegistry` enum.

`KeyId::new(String)` requires exactly 20 Unicode scalar values and
`KeySecret::new(String)` requires exactly 40. Alphabets are not checked, and
strings are not normalized. The same validation applies during deserialization.
`KeySecret` and `Credential` redact the secret in `Debug`.

`register_key` atomically rejects any existing ID with `KeyAlreadyExists`.
`get_key` returns `None` for an absent key. `revoke_key` removes the record and
succeeds if it is already absent. Successful mutations await a durable flush;
a flush or transport error does not imply rollback. Secrets are stored as
provided, without encryption at rest.

`list_keys(principal)` returns the principal's `Vec<KeyId>` in lexicographic
order, without secrets. Local listing scans the existing tree and has no
snapshot guarantee during concurrent mutations. Remote listing uses a unary
gRPC response subject to the 4 MiB message limit.

```rust
use s3_credentials_registry::{KeyId, KeySecret, LocalRegistry, traits::S3CredentialRegistry as _};

async fn example() -> Result<(), s3_credentials_registry::Error> {
    let registry = LocalRegistry::new(sled::open("./data/s3-credentials-registry")?)?;
    let id = KeyId::new("A".repeat(20))?;
    let secret = KeySecret::new("s".repeat(40))?;
    registry.register_key(id.clone(), secret, 42).await?;
    let credential = registry.get_key(&id).await?;
    registry.revoke_key(&id).await?;
    Ok(())
}
```

Features default to local-only. `cli` exposes `S3CredentialsRegistryConfig` for
`#[command(flatten)]`. `grpc-client` enables `RemoteRegistry::connect(Endpoint)`;
`grpc-server` enables `GrpcService`. `grpc-tls` enables optional TLS. Default
builds do not require tonic or protoc; gRPC builds require protoc.

```sh
cargo run -p s3-credentials-registry --features cli,grpc-server -- \
  --listen 127.0.0.1:50055 \
  --s3-credentials-registry-path ./data/s3-credentials-registry
```

The default listen address is `127.0.0.1:50055`. Ctrl-C requests graceful
shutdown. With `grpc-client`, `--s3-credentials-registry-endpoint` selects a
remote registry without opening the local database, including when serving a
relay. Client calls use a shared channel, with no application retries or local
fallback. Both directions limit messages to 4 MiB.

The CLI reads `WD40_S3_CREDENTIALS_REGISTRY_PATH`,
`WD40_S3_CREDENTIALS_REGISTRY_ENDPOINT` and, with client TLS,
`WD40_S3_CREDENTIALS_REGISTRY_CA`. The CA option requires HTTPS. Servers accept
`--tls-cert` and `--tls-key` with `grpc-tls`. TLS, including upstream client TLS,
follows the other registries' configuration.

This crate stores credentials. Key generation, S3 request authentication and
policy evaluation belong to the caller.
