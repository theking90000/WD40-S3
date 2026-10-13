# access-controller

Evaluate key permissions through the existing `PolicyRegistry` Local/Remote enum.
The controller loads policies and compiles them; it does not authenticate,
validate identities, enumerate objects or enforce storage operations.
The application must check the decision before performing each protected action.

```rust
use access_controller::{AccessController, Action, Decision, Principal};

let access = AccessController::new(registry);
let principal = Principal::Authenticated(alice);
let decision = access.authorize(principal, Action::Read, "photos/a.jpg").await?;
if decision == Decision::Deny {
    // Return the adapter's access-denied response before reading the object.
}

// Reuse one immutable permission snapshot for a batch or listing.
let compiled = access.compile(principal).await?;
let revision = compiled.revision();
let visible = compiled.authorize(Action::List, "photos/a.jpg");
```

`AccessController` owns the delegating `PolicyRegistry` enum, following the other
WD40 systems. It is cloneable and re-exports `PolicyRegistry` for construction.
Construction starts no tasks and performs no I/O. The library requires no async
runtime of its own; the selected registry determines runtime requirements.
Futures are Send. `CompiledPolicy` is Send + Sync and can outlive the controller
and registry.

## Policy semantics

Anonymous principals load Everyone. Authenticated principals load Everyone,
Authenticated and Specific(id). All selectors are passed to a single
`load_policies()` call, which returns their deduplicated union and revision from
one coherent read. The engine never uses separate association/document reads.

Permissions combine independently of statement or policy order. At least one
matching Allow is required. Any matching Deny for the requested action takes
precedence, including a parent-prefix Deny above a more specific Allow.
The engine uses the registry's existing Read, Write, Delete and List actions.
Protocol operations and permissions on historical versions are the adapter's
responsibility; this model only distinguishes actions and keys.

Exact matches equality and Prefix matches `starts_with`, including the prefix
itself. Strings are opaque UTF-8, without normalization, wildcard expansion or
path parsing. `*`, `.`, `/` and NUL remain literal. Prefix("") matches every key;
Exact("") matches only the empty key. Empty policies and action masks are valid.
Storage may impose stricter key validity without changing policy matching.

## Compilation and evaluation

`CompiledPolicy::from_snapshot(snapshot, limits)` is the compiler for an already
loaded `PolicySnapshot`. It consumes resource buffers without cloning strings.
A compressed byte radix trie stores separate exact and prefix Allow/Deny masks.
Repeated rules merge their masks. Edge splits use ranges into owned buffers,
including ranges that cross UTF-8 character boundaries. All traversal operates
on bytes. Nodes reside in a flat arena, including during destruction.

Synchronous evaluation performs no I/O or allocation and takes O(key byte
length). Children are sorted by their first byte and searched by binary search;
each node has at most 256 children. For a batch, evaluation costs
O(sum of the evaluated keys' byte lengths). No object count enters evaluation.
Compilation depends on the number of statements and total resource lengths.

`CompilationLimits` defaults to 100,000 statements, 16 MiB of summed resource
buffer capacities and 200,001 nodes, including the root. Configure limits with
`AccessController::with_limits`. Repetitions and empty action rules count toward
the input limits. Buffer capacities count so excess capacity cannot bypass the
resource memory bound. Index memory is bounded by those buffers, source entries,
nodes and edges, with vector capacity and allocator overhead. Limits apply after
loading; they do not bound memory already allocated by the registry.

Compiler vector reservations are fallible. `Error` preserves registry and
reservation causes, and distinguishes compilation limits and internal index
errors. Typed policies have no additional invalid-policy syntax to reject.
Technical failures propagate separately from Decision::Deny and never produce
Decision::Allow. Standard allocation of the returned Arc follows Rust's usual
allocation behavior; the crate does not promise recovery from every process OOM.

## Coherence and LIST

Each `authorize()` loads a fresh coherent snapshot. `compile()` returns an
`Arc<CompiledPolicy>` whose revision and permissions remain unchanged after
updates or revocations. A revision identifies the loaded state of that registry;
it does not refresh an index or support historical lookup. There is no cache,
TTL, invalidation service or immediate-revocation guarantee for retained values.
An operation reusing a compiled policy continues under that loaded snapshot.

For LIST, the adapter filters each returned record by
`compiled.authorize(Action::List, &record.key)`. An exact grant for one child can
make that child visible even when the requested prefix itself has no grant.
Do not require a preliminary List decision on that prefix. Admission of the
listing request as a whole remains an adapter decision.

The current ObjectRegistry returns pages with opaque continuation cursors, not
a seekable key cursor. Filtering a complete source page can produce fewer
results or an empty page with a continuation. The adapter must preserve
continuation even in that case. Filling a page to a visible-result limit needs a
separate continuation contract if it stops inside a source page. Object pages
are not snapshots, even when permissions are reused from one compiled snapshot.
Range enumeration and specialized listing engines are outside this version.

## Tests and benchmarks

Conformance tests cover empty rules, exact/prefix contradictions, literal keys,
UTF-8 edge splits, limits, long keys and shared/nested prefixes. Reproducible
generated policies and requests compare the trie with a naive interpreter and
reversed insertion order across 1,000 generated cases. Concurrency tests share a
compiled snapshot across sixteen threads, load snapshots concurrently, inject
registry failures and mutate policies while checking revision consistency.

The standalone benchmark separates registry loading, compilation and batches
of synchronous evaluations. It covers policies shared between principals,
common prefixes and deeply nested prefixes. Fixture preparation, snapshot
cloning and result destruction occur outside timed operations. Registry setup
uses a temporary local database; it performs no network I/O.

```sh
cargo test -p access-controller
cargo bench -p access-controller --bench authorization
```
