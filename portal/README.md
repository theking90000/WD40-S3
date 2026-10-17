# Portal

Minimal portal service with a Vite and React Router framework console, prerendered
during compilation. OIDC, organizations and WD40 RPC operations are not
implemented yet. `GET /api/health` returns `ok`.

## Standalone

Install Node.js 22.22+ with npm, then run from the repository root:

```sh
cargo run -p portal --features standalone
```

Open `http://127.0.0.1:8080/`. Use `-- --listen 127.0.0.1:8081` or
`WD40_PORTAL_LISTEN` to change the address.

Cargo runs `build.rs` to execute `npm ci` and `npm run build`. Vite and React Router
generate the prerendered pages and browser assets under `console/build/client/`.
`rust-embed` embeds that directory in debug and release builds. Build failures stop
compilation. No manual npm step or pre-existing build directory is needed.

`node_modules/`, `build/` and `.react-router/` are generated under `console/` and
ignored by Git. The npm download cache remains under Cargo's build directory.

To produce the executable:

```sh
cargo build -p portal --release --features standalone
```

The executable serves its embedded console without Node.js, npm or external assets.
React runs on Node.js only during compilation. No JavaScript server runs in
production.

## Pages and prerendering

The console uses React 19.3.1, React Router 8.4.0 and Vite 8.3.4.
`console/react-router.config.js` sets `ssr: false` and `prerender: true`, following
the [React Router prerendering guide](https://reactrouter.com/how-to/pre-rendering).
The framework prerenders all static paths and generates browser hydration code.
Dynamic routes require concrete URLs in React Router's `prerender` configuration.

`console/src/routes.js` uses the official `@react-router/fs-routes` package to
discover pages under `console/src/routes/`. `_index.jsx` maps to `/` and
`status.jsx` to `/status`. Adding `settings.jsx` generates `/settings/index.html`
on the next Cargo build, without editing the route configuration. Nested and
dynamic routes follow the [file route conventions](https://reactrouter.com/how-to/file-route-conventions).

`npm run build` invokes the official `react-router build` command, which uses Vite
to build and prerender the application. The default Web Streams rendering entry
requires `isbot` during compilation; no `@react-router/node` adapter is needed.

`console/src/root.jsx` defines the document and navigation. Route modules provide
their pages as default exports. Page content is present before browser JavaScript
runs; React hydrates it for interactions and React Router handles navigation.
Vite and React Router handle asset hashing, HTML references, tree shaking and
route code splitting. There is no custom HTML template, route traversal or
prerender script.

The complete `build/client/` directory, including generated data files, is embedded
and served by path with MIME metadata from `rust-embed`. Adding generated files does not
require changing Rust routes. The temporary server build is not embedded.

Hashed files under `assets/` use `Cache-Control: public, max-age=31536000, immutable`.
Other files use `Cache-Control: no-cache`. `/status` and `/status/` both serve
`status/index.html`. A GET/HEAD fallback serves files after the API routes;
missing files and unknown routes return 404. There is no SPA navigation fallback.

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

## Rust architecture

- `lib.rs` declares modules and public exports; `portal.rs` assembles the service.
- `api/mod.rs` assembles API routes. Group new features under `api/<feature>/`,
  with routes in `mod.rs` and one file per operation as the feature grows.
- Keep each handler, its request/response types and operation-specific logic together.
  Extract shared logic within the feature only when needed; reuse existing WD40 crates.
- Keep modules private by default. `assets.rs` serves the console; `cli.rs` and
  `main.rs` handle CLI arguments and standalone startup.

## Deployment prefix

The default public path is `/`. For a prefix, set `WD40_PORTAL_BASE_PATH` during
compilation, with both leading and trailing slashes:

```sh
WD40_PORTAL_BASE_PATH=/portal/ cargo build -p portal --release --features standalone
```

The standalone executable mounts the service at that prefix. When embedding the
library in Axum, use `nest_service` to mount its router under the same prefix,
including requests to the prefix root with a trailing slash. Open `/portal/` with
the trailing slash. This build-time setting configures React Router's basename,
Vite's asset URLs, API calls and embedded HTML lookup together; it is not a runtime
environment setting.
