# policy-registry

Store policies and principal bindings in sled, with postcard records and a
persistent global revision. The crate follows the other registries with
`traits::PolicyRegistry`, `LocalRegistry`, the delegating `PolicyRegistry` enum,
optional gRPC client/server transports, TLS and composable CLI configuration.

```rust
use policy_registry::{
    Action, Effect, LocalRegistry, Policy, PrincipalSelector, ResourcePattern,
    Statement, traits::PolicyRegistry as _,
};

let registry = LocalRegistry::new(database)?;
let id = 42_u128;
let policy = Policy {
    statements: vec![Statement {
        effect: Effect::Allow,
        actions: vec![Action::Read, Action::List],
        resource: ResourcePattern::Prefix("photos/".to_owned()),
    }],
};
let revision = registry.put_policy(id, policy).await?;
registry.attach(PrincipalSelector::Specific(alice), id).await?;
let snapshot = registry.load_policies(&[
    PrincipalSelector::Everyone,
    PrincipalSelector::Authenticated,
    PrincipalSelector::Specific(alice),
]).await?;
```

## Model and operations

`PolicyId` and `PrincipalSelector::Specific` use caller-supplied `u128` IDs.
`Actions` is `Vec<Action>`, with `Read`, `Write`, `Delete` and `List` variants.
Policies retain their statements, action order, repetitions and resource strings
as supplied, including empty lists and empty resource patterns.
The registry stores both Allow and Deny statements without interpreting them.
Authorization, resource matching and action evaluation belong to the caller.

`get_policy` returns `None` for an absent policy. `put_policy` creates or replaces
the policy while preserving existing bindings. `attach` requires an existing
policy and returns `Error::PolicyNotFound` otherwise. `detach` succeeds for a
missing binding, even if the policy itself is absent. `delete_policy` atomically
removes the policy and every binding to it. Recreating the same ID does not
restore its previous bindings.

`load_policies` loads the union of policies attached to exactly the selectors
provided. It does not infer Everyone or Authenticated from Specific. Include
the applicable selectors explicitly. The result is
`PolicySnapshot { revision: Revision, policies: Vec<(PolicyId, Policy)> }`.
Each policy appears once, in ascending ID order. An empty selector list returns
an empty policy list with the current revision. The revision and all policies
come from one read transaction, including across independent handles sharing
the same database.

## Revisions and persistence

`Revision(pub u64)` starts at zero. Each successful state change increments it
once, inside the mutation transaction. Identical puts, repeated attachments,
missing detachments and deletion of missing policies retain the current
revision. Revision overflow aborts the entire mutation with `RevisionOverflow`.
Revisions survive database reopening and apply to the whole registry, so an
unrelated policy change also invalidates a previously loaded revision.
The registry retains current policies only; a revision is not a historical
policy lookup.

Construct the local registry with a caller-owned `sled::Db`. It opens three
trees: `policy-records`, `policy-bindings` and `policy-state`. Policy keys are
16-byte big-endian IDs. Bindings contain both selector-to-policy sets and
policy-to-selector sets in separate key namespaces. The reverse index lets
deletion remove all bindings using transactional lookups without scans.
Mutations update both indexes and the revision atomically. Clones and separate
handles over the same database require no additional application mutex.

Every successful mutation awaits a durable database flush, including operations
that retain the revision. As in the other registries, a dedicated thread runs
`Db::flush` and returns its result through a oneshot channel, avoiding sled's
shared I/O-pool starvation. No async runtime is required for Local.
A flush error or future cancellation can occur after the transaction commits;
an error does not imply rollback. `Error` preserves database and serialization
causes and is also exported as `RegistryError`.

## Optional gRPC and CLI

The default feature set is empty. `grpc-client` enables `RemoteRegistry` and
`PolicyRegistry::Remote`. `grpc-server` enables
`GrpcService::new(registry).into_server()`, including forwarding through a
Remote registry. `grpc-tls` adds TLS to enabled transports. `cli` enables
`PolicyRegistryConfig` independently; the server binary requires both `cli`
and `grpc-server`. gRPC builds require `protoc` in PATH or via `PROTOC`.

```sh
cargo run -p policy-registry --features cli,grpc-server -- \
  --listen 0.0.0.0:50054 --policy-registry-path ./data/policy-registry
```

The default listen address is `127.0.0.1:50054`. Ctrl-C requests graceful
shutdown. `--policy-registry-path` or `WD40_POLICY_REGISTRY_PATH` selects the
local directory, defaulting to `./data/policy-registry`. With `grpc-client`,
`--policy-registry-endpoint` or `WD40_POLICY_REGISTRY_ENDPOINT` selects Remote
without opening a local database. Clients require a Tokio runtime and connect
with `RemoteRegistry::connect(tonic::transport::Endpoint)`.

With `grpc-tls`, paired `--tls-cert` and `--tls-key` PEM files enable incoming
TLS. HTTPS clients verify certificates and hostnames using system trust roots.
`--policy-registry-ca` or `WD40_POLICY_REGISTRY_CA` adds a PEM CA. HTTPS requires
the TLS feature, and a custom CA requires an HTTPS endpoint.

The `wd40.policy_registry.v1` protocol exposes all six trait methods as unary
RPCs. Policies and selectors use typed Protobuf messages. IDs use exactly
16 big-endian bytes; revisions use native `uint64`. Missing fields, malformed
IDs and unknown enum values are rejected. Messages have a 4 MiB encoding and
decoding limit. Structured status details preserve `PolicyNotFound` and
`RevisionOverflow`; other errors remain `Error::Rpc`. Remote clones share their
channel; failed calls are not replayed and never fall back to Local. The
transport preserves the registry's snapshot and durable-flush contracts.

## Tests

Local integration tests cover the policy lifecycle, idempotent revisions,
explicit selector unions, binding cleanup, empty policies, serialization,
database reopening, revision-overflow rollback and corrupt indexes.
Concurrent tests check that independent handles return matching policies and
revisions, preserve parallel binding updates and flush sixteen databases
without starving sled's I/O pool.

gRPC tests cover direct clients and relays, domain errors, malformed requests,
message-size limits, shutdown, CLI selection and TLS trust/hostname checks.
The concurrency test uses 32 clients over shared and independent servers on
a Tokio runtime with sixteen workers.

```sh
cargo test -p policy-registry
cargo test -p policy-registry --all-features -- --test-threads=16
cargo test --workspace --all-features -- --test-threads=64
```
