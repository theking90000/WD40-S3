# Portal

Portal service with OIDC authentication and a Vite and React Router framework
console, prerendered during compilation. The home page provides login and logout
controls through the authentication API. The organizations API and WD40 RPC
operations are not implemented yet. `GET /api/health` returns `ok` without
authentication.

## Standalone

Install Node.js 22.22+ with npm, configure a confidential OIDC client, then run
from the repository root:

```sh
export WD40_PORTAL_OIDC_ISSUER_URL=https://identity.example.com
export WD40_PORTAL_OIDC_CLIENT_ID=wd40-portal
# Supply WD40_PORTAL_OIDC_CLIENT_SECRET through your secret management.
cargo run -p portal --features standalone
```

Open `http://127.0.0.1:8080/`. Use `-- --listen 127.0.0.1:8081` or
`WD40_PORTAL_LISTEN` to change the address.

The server opens the portal database and applies its versioned migrations before
starting HTTP. `--database-url` or `WD40_PORTAL_DATABASE_URL` selects the database.
The default, `sqlite://portal.db?mode=rwc`, opens or creates `portal.db` in the
working directory. Connection or migration failures stop startup. The server
closes the database after HTTP requests have drained on graceful shutdown, and
also on startup or serving errors after connection setup.

SQLite is enabled by default. To build for PostgreSQL only, use
`--no-default-features --features standalone,postgres` and supply a PostgreSQL
database URL at runtime. Both drivers can be enabled with `--features postgres`.

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
`console/react-router.config.ts` sets `ssr: false` and `prerender: true`, following
the [React Router prerendering guide](https://reactrouter.com/how-to/pre-rendering).
The framework prerenders all static paths and generates browser hydration code.
Dynamic routes require concrete URLs in React Router's `prerender` configuration.

`console/src/routes.ts` uses the official `@react-router/fs-routes` package to
discover pages under `console/src/routes/`. `_index.tsx` maps to `/` and
`status.tsx` to `/status`. Adding `settings.tsx` generates `/settings/index.html`
on the next Cargo build, without editing the route configuration. Nested and
dynamic routes follow the [file route conventions](https://reactrouter.com/how-to/file-route-conventions).

`npm run build` invokes the official `react-router build` command, which uses Vite
to build and prerender the application. The default Web Streams rendering entry
requires `isbot` during compilation; no `@react-router/node` adapter is needed.

The console uses strict TypeScript. From `console/`, `npm run typecheck` generates
React Router's route types and runs TypeScript without emitting files. This check
is separate from the Cargo-driven production build.

`console/src/root.tsx` defines the document and navigation. Route modules provide
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

`Portal::new(config, db).await` accepts a migrated `portal_db::DatabaseConnection`
and discovers the configured OIDC provider and its public keys. It does not open
a listener. Discovery and configuration failures return `std::io::Error`.
The caller owns the runtime, database connection setup, migrations and shutdown.
`into_router()` returns an `axum::Router`; `into_service()` returns a
`hyper_util::service::TowerToHyperService<axum::Router>` for Hyper integration.

```rust
use portal_db::{Database, Migrator, MigratorTrait};

let db = Database::connect(database_url).await?;
Migrator::up(&db, None).await?;
let portal = portal::Portal::new(
    portal::OidcConfig {
        issuer_url,
        client_id,
        client_secret,
    },
    db.clone(),
).await?;
let service = portal.into_service();
// Serve requests, then call db.close().await after they have drained.
```

The API router supplies the connection as `State<portal_db::DatabaseConnection>`
for database-backed handlers. Authentication routes retain their own OIDC state.

Features default to `sqlite`; `postgres` enables the PostgreSQL driver.
`cli` exposes composable `PortalArgs`. `embed-console`
builds and embeds the console for library consumers too. `standalone` enables
both features and the executable. Builds without `embed-console` do not run npm.

## OIDC configuration

The standalone server requires three OIDC environment variables, also available
as `--oidc-issuer-url`, `--oidc-client-id` and `--oidc-client-secret`:

| Variable | Value |
| --- | --- |
| `WD40_PORTAL_OIDC_ISSUER_URL` | Exact issuer advertised by the provider, including its path if any. |
| `WD40_PORTAL_OIDC_CLIENT_ID` | Confidential client ID for this portal. |
| `WD40_PORTAL_OIDC_CLIENT_SECRET` | Client secret, used only by Rust. |

These are runtime settings. They never enter the console build. The client uses
Authorization Code with PKCE S256 and requests `openid profile email`. Configure
the client's allowed callback URLs and any additional default scopes in Rauthy.
There is no portal redirect-URI environment variable: React supplies its public
callback URL for each login, including the deployment prefix when applicable.
The provider enforces its callback allowlist. The same URI must be supplied again
when exchanging the code.

Discovery must advertise token, UserInfo and introspection endpoints. Client
authentication uses HTTP Basic. Issuer and endpoint URLs must use HTTPS, except
for HTTP loopback development. The shared asynchronous HTTP client refuses
redirects and uses a 5-second connection timeout and 10-second request timeout.
Discovery keys are held for the life of the service; restart the portal to fetch
new keys if the provider rotates signing keys.

## Authentication API

Console and API must share an origin. Every POST below requires the header
`X-WD40-Portal: 1`. Login and callback also require `Content-Type: application/json`.
Cross-origin browser requests are not supported: do not wrap these routes in a
credentialed CORS policy allowing other origins. When present, `Sec-Fetch-Site`
must be `same-origin` or `none`. All authentication responses use
`Cache-Control: no-store`.

### POST /api/login

```json
{ "redirect_uri": "https://console.example.com/portal/auth/callback" }
```

Returns `200` with `{ "url": "https://identity.example.com/..." }`. React navigates
to that URL. Rust generates independent random state, nonce and PKCE verifier
values and sets three temporary HttpOnly cookies, valid for five minutes. These
cookies contain the random values directly; they are neither encrypted nor backed
by server storage. The PKCE verifier stays out of the authorization URL.

Callback URLs must be absolute HTTPS URLs, or HTTP loopback URLs for development,
without credentials or fragments. The console detects callbacks globally on any
served page; there is no dedicated callback route. Each selected callback URI
must be allowed by the provider.

### POST /api/callback

After Rauthy redirects to the React page with `code` and `state`, React sends:

```json
{
  "code": "code-from-rauthy",
  "state": "state-from-rauthy",
  "redirect_uri": "https://console.example.com/portal/auth/callback"
}
```

React does not generate or retain a nonce, PKCE verifier or challenge. The browser
sends the HttpOnly cookies automatically. Rust compares the supplied state with
the cookie, exchanges the code using the cookie's verifier, and validates the ID
token's signature, issuer, audience, expiry and nonce. It also checks `azp` and
`at_hash` when present, and introspects the access token to verify its subject and
client binding before setting the authentication cookie.

Success returns `204` and an HttpOnly cookie containing only the access token.
No token appears in the response body. The temporary state, nonce and PKCE cookies
are expired on every callback response, including malformed JSON, mismatched state
and provider failures. React can then fetch `/api/me` and navigate to its desired
page. If Rauthy returns an authorization error instead of a code, submitting that
failure to this endpoint returns `400` and also clears the temporary cookies.

Production cookies are named `__Host-wd40_oidc_state`, `__Host-wd40_oidc_nonce`,
`__Host-wd40_oidc_pkce` and `__Host-wd40_oidc_access_token`. They use `Secure`,
`HttpOnly`, `SameSite=Lax`, `Path=/` and no `Domain`. HTTP loopback development
uses the same names without `__Host-` and permits cookies without `Secure`.
Production never accepts these development cookies. Reverse proxies must
preserve the public `Host`; forwarded headers are not trusted for this decision.
All cookies belong to the origin, including when mounted under `/portal/`.
Starting another login on the same origin replaces the pending login cookies.

The access-token cookie lifetime is bounded by the provider's expiration. Tokens
whose serialized cookie exceeds 4096 bytes are rejected. Refresh tokens are not
stored; start another login when the access token expires.

### GET /api/me

Requires the access-token cookie. Authentication middleware performs an
introspection request, checks token activity, expiry, subject and client binding,
then fetches UserInfo and checks that its subject matches. No user or token cache
is maintained. The provider must return `exp`, `sub`, and `client_id` or an audience
containing the configured client ID. Any additional issuer, client ID or audience
returned must also match. A token explicitly identified as a non-Bearer token is
rejected.

The handler returns the `CurrentUser` placed in request extensions:

```json
{
  "issuer": "https://identity.example.com",
  "subject": "user-subject",
  "name": "Alice",
  "email": "alice@example.com",
  "email_verified": true,
  "roles": [],
  "groups": []
}
```

`name`, `email` and `email_verified` may be null. Roles and groups are read when
included in UserInfo; absent claims produce empty lists. They do not grant portal
permissions automatically. Identity is the `(issuer, subject)` pair; email is
display data. Profiles and memberships remain owned by the provider. Their
freshness and revocation behavior depend on the provider's introspection and
UserInfo implementations.

Add protected routes to the same router group as `/api/me` in `api/auth/mod.rs`;
handlers can extract `Extension<portal::CurrentUser>`. Assets and health remain
public.

### POST /api/logout

Requires `X-WD40-Portal: 1`, including when no authentication cookie is present.
Returns `204`, expires the access-token cookie and clears pending login cookies.
It does not log out of Rauthy or revoke a copied token.

### Errors

Errors return JSON such as `{ "error": "unauthenticated" }`:

| Status | Meaning |
| --- | --- |
| `400` | Malformed callback/login input, or invalid authorization code. |
| `401` | Missing or mismatched state, invalid ID token, or missing/invalid access token. |
| `403` | Missing browser-request header or rejected fetch origin context. |
| `413` | Authentication request body exceeds 16 KiB. |
| `502` | Invalid or unsupported provider response, including missing required claims. |
| `503` | Provider request failed or timed out. |

Invalid authentication cookies are expired on `401`. Provider outages preserve
the access-token cookie. JSON extractor rejections on login retain Axum's status
with a JSON error body; the callback normalizes malformed input to JSON `400` so
it can always expire its temporary cookies. No authentication payload or provider
error body is logged.

## Console authentication

`console/src/auth.ts` owns the browser protocol and API calls without importing
React or React Router. It exports:

- `getLoginUrl(returnTo?, { signal }?)`, which requests an authorization URL and
  saves the exact callback URI and return destination in `sessionStorage`.
- `login(returnTo?)`, which calls `getLoginUrl` and navigates the browser to the
  provider. The default destination is the current page, including query and hash.
- `completeCallback({ code, state, redirectUri })`, the low-level callback POST.
- `getCurrentUser({ signal }?)`, which returns a user or `null` on `401` and
  propagates other failures.
- `logout()`, which clears the server cookies and pending browser login metadata.
  The caller chooses the subsequent navigation, typically to a public route.

Requests use the compiled base path, same-origin credentials and `no-store`.
POSTs include `X-WD40-Portal: 1`; JSON bodies also set `Content-Type`.
`AuthError` exposes an error `code` and HTTP `status`. Network errors propagate.
`CurrentUser` describes the `/api/me` response; `getCurrentUser` returns
`Promise<CurrentUser | null>`.
No access token, nonce or PKCE verifier is exposed to JavaScript. The callback's
returned state is read from the URL and forwarded to Rust for its cookie
comparison; it is not persisted in browser storage.

The only browser storage is `{ redirectUri, returnTo }` for a pending login,
scoped to the console base path in `sessionStorage`. The fragment is excluded
from the callback URI and restored after success. The original URI is reused
verbatim for the token exchange, including its original query encoding. Return
destinations must remain on the current origin and under the console base path.
Callback parameters `code`, `state`, `iss`, `session_state`, `error`,
`error_description` and `error_uri` are reserved for OIDC and cannot appear in a
login return destination. Only one login can be pending per origin because the
Rust cookies are shared by tabs and mounts.

### Global callback handling

`root.tsx` exports a `clientMiddleware` from `auth-router.ts`. Before route
loaders or actions run, it detects `code`/`state` or an OIDC error on any route,
finishes the callback, and replaces the URL with its clean destination. Existing
business query parameters and the saved fragment are preserved. A root build-time
`loader` and browser `clientLoader` both return `null`; `clientLoader.hydrate = true`
ensures the middleware runs on the first document load without fetching a user
or removing public prerendered content.

The same callback shares one promise, including across repeated middleware
calls. Its POST is not aborted when a route navigation is cancelled. A navigation
that supersedes an ongoing callback waits for its exchange before loading data.
Malformed callbacks, missing browser metadata and authorization refusals trigger
a best-effort empty callback POST to expire Rust's temporary cookies. Once an
exchange has been submitted, it is not retried automatically.

Both success and failure remove pending browser metadata and replace the callback
URL. On failure, the router then receives the original error before protected
loaders can start another login. Route error boundaries can handle it when their
UI is built. No component effect or dedicated callback page is involved.

### Public and protected routes

The home and status pages remain public. The home page uses `optionalUser` to
call `/api/me` and display login or logout controls. Logout revalidates its loader
to update the displayed user. The status page does not fetch a user.
`auth-router.ts` provides `optionalUser(args)`, `requireUser(args)` and
`withUser(loader?)` for routes that need identity. A fresh React Router context
shares a lazy user promise and a lazy login promise during each navigation or
fetcher call. There is no persistent user cache or React authentication provider.

For a route with an optional user:

```ts
import type { LoaderFunctionArgs } from "react-router";
import { optionalUser } from "../auth-router.ts";

export async function clientLoader(args: LoaderFunctionArgs) {
  return { user: await optionalUser(args) };
}

export function HydrateFallback() { return null; }
```

For a protected route:

```ts
import { withUser } from "../auth-router.ts";

export const clientLoader = withUser(async ({ user, request }) => {
  // Load this route's data here, after authentication has completed.
  return { user };
});

export function HydrateFallback() { return null; }
```

`withUser()` without an argument returns `{ user }`. The wrapper runs during
initial hydration and only calls the supplied loader after obtaining a user.
Without a user it starts one login and uses `redirectDocument` to navigate to the
provider. Provider outages and other errors propagate instead of triggering a
login. Every child loader that depends on identity needs its own guard; parent
and child loaders can run in parallel. Client actions can call `requireUser(args)`
before their mutations. Rust remains responsible for API access control.

Components can consume the result through `loaderData` or `useLoaderData` when
the UI is added. No authenticated content or user lookup runs during prerendering.
Any route used for a document callback must be served by Rust. Dynamic paths not
included in prerendering currently return `404`; they need explicit prerendered
paths or a future server fallback before they can receive an OIDC return.

The focused integration check is available from `portal/console/` with
`node --test src/auth.test.ts`. Node.js strips the TypeScript types directly.
It uses mocked HTTP and a memory router, without
an IdP or browser service.

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
