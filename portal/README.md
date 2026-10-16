# Portal

Minimal portal service with a React console. OIDC, organizations and WD40 RPC
operations are not implemented yet. `GET /api/health` returns `ok`.

## Standalone

Install Node.js 18 or newer with npm, then run from the repository root:

```sh
cargo run -p portal --features standalone
```

Open `http://127.0.0.1:8080/`. Use `-- --listen 127.0.0.1:8081` or
`WD40_PORTAL_LISTEN` to change the address.

Cargo runs `build.rs`, which copies the console sources into `OUT_DIR`, installs
the locked npm dependencies with `npm ci`, builds the console and embeds its
HTML, JavaScript and CSS. Build failures stop compilation. Source changes trigger
a rebuild; generated files and the npm cache remain under Cargo's build directory.
No manual npm step or pre-existing `dist/` directory is needed.

To produce the executable:

```sh
cargo build -p portal --release --features standalone
```

The executable serves its embedded console without Node.js, npm or external assets.

The complete `dist/` directory is embedded and served by path, with inferred MIME
types. Adding generated chunks or assets does not require changing Rust routes.
esbuild explicitly enables tree shaking, minification and ESM code splitting;
dynamic `import()` calls produce separate chunks. Entry points, chunks and imported
assets have content hashes in their names. The build generates the HTML references
from esbuild's metadata.

Hashed files under `assets/` use `Cache-Control: public, max-age=31536000, immutable`.
HTML uses `Cache-Control: no-cache`. Missing files return 404; there is no SPA
navigation fallback.

## Library

`Portal::new()` constructs the service without opening a listener or starting tasks.
`into_router()` returns an `axum::Router`; `into_service()` returns a
`hyper_util::service::TowerToHyperService<axum::Router>` for Hyper integration.

```rust
let portal = portal::Portal::new();
let service = portal.into_service();
```

Features default to empty. `cli` exposes composable `PortalArgs`. `embed-console`
builds and embeds the console for library consumers too. `standalone` enables
both features and the executable. Builds without `embed-console` do not run npm.

The console uses relative URLs and can be nested under a fixed Axum prefix.
When nested at `/portal`, open `/portal/` with the trailing slash.
