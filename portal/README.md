# Portal

Portal service with OIDC authentication and a Vite and React Router framework
console, prerendered during compilation. The Rust authentication API is available;
the console login flow, organizations API and WD40 RPC operations are not
implemented yet. `GET /api/health` returns `ok` without authentication.

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

`Portal::new(config).await` discovers the configured OIDC provider and fetches its
public keys, without opening a listener or starting background tasks. Discovery
and configuration failures return `std::io::Error`. The caller owns the runtime.
`into_router()` returns an `axum::Router`; `into_service()` returns a
`hyper_util::service::TowerToHyperService<axum::Router>` for Hyper integration.

```rust
let portal = portal::Portal::new(portal::OidcConfig {
    issuer_url,
    client_id,
    client_secret,
}).await?;
let service = portal.into_service();
```

Features default to empty. `cli` exposes composable `PortalArgs`. `embed-console`
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
without credentials or fragments. The React callback page will be implemented
separately and must be included in the console's prerendered routes.

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
