# Portal

Portal service with OIDC authentication and a Vite and React Router framework
console, with static pages prerendered during compilation. After login, the home
page selects or creates an organization. The organizations API lists memberships
and creates organizations. WD40 RPC operations are not implemented yet.
`GET /api/health` returns `ok` without authentication.

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
`console/react-router.config.ts` sets `ssr: false` and `prerender: ["/", "/login"]`, following
the [React Router prerendering guide](https://reactrouter.com/how-to/pre-rendering).
The framework prerenders these static paths and generates browser hydration code.
Dynamic routes require concrete URLs in React Router's `prerender` configuration.

`console/src/routes.ts` declares the login route, an authenticated session layout,
and an organization layout. `/` selects or creates an organization; `/login`
starts authentication. Application pages require an organization: `/{organization_id}/`
is its overview and `/{organization_id}/members` lists organization members.
`/{organization_id}/status` remains available directly but is not linked from the
navigation. Buckets, access keys and settings are visible but disabled.

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
Other files use `Cache-Control: no-cache`. A GET/HEAD fallback serves files after
the API routes. Recognized organization URLs, with a UUID and an optional `/status` or `/members`
suffix, serve React Router's SPA document for direct loads and refreshes. This is
`__spa-fallback.html`, or the root `index.html` when building with a deployment
prefix, whose prerendered pages live below that prefix.
Organization IDs are checked against the caller's memberships in the browser
before application pages load. Missing files, API paths and unknown routes return 404.

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
permissions automatically. Identity is the `(issuer, subject)` pair. Portal
organization memberships are stored separately in the portal database and matched
by verified email. Profile claims, roles and groups come from the provider; their
freshness and revocation behavior depend on its introspection and UserInfo
implementations.

Wrap protected feature routers with `auth::protect` in `api/mod.rs`; handlers
can extract `Extension<portal::CurrentUser>`. The wrapper authenticates requests
and applies `Cache-Control: no-store`, including errors. Assets and health
remain public.

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

## Organizations API

All endpoints require the authentication cookie and a non-empty email with
`email_verified: true` in the provider's UserInfo. Memberships use the email
exactly as returned, without case conversion. Changing that email changes which
memberships are accessible. Provider roles and groups do not grant organization
access.

### GET /api/organizations

Returns `200` with an array of organizations where the user is a `MEMBER` or
`OWNER`, ordered by creation date descending and UUID ascending for ties:

```json
[
  {
    "id": "e2b219e2-f69d-41cd-83b2-e3e791133b24",
    "name": "My organization",
    "created_at": "2026-10-18T12:00:00+00:00",
    "access": "OWNER"
  }
]
```

Users with no memberships receive `[]`. A single inner join retrieves only the
organization fields and the caller's access; other members' emails are not
returned. There is no pagination in this initial version.

### POST /api/organizations

Send `Content-Type: application/json` and `X-WD40-Portal: 1` with:

```json
{ "name": "My organization" }
```

Only `name` is accepted. Leading and trailing whitespace is removed; the result
must contain 1 to 128 Unicode characters. Names need not be unique.

The organization and the creator's `OWNER` membership are inserted in one
transaction. UUID and creation timestamps are generated by the existing entity
and database defaults. Success returns `201` with the same object shape as a
listing entry. Failed or cancelled transactions are rolled back.

POST and DELETE require `X-WD40-Portal: 1` and, when present, `Sec-Fetch-Site` must be
`same-origin` or `none`. Rejected browser requests return `403 csrf_rejected`
before database operations. GET does not require the custom header.

The portal does not grant cross-origin access through CORS: it emits no
`Access-Control-Allow-Origin` or `Access-Control-Allow-Credentials` headers and
does not authorize preflight requests. Browsers require a successful preflight
before sending this custom header to another origin. Keep this policy when
embedding the router or configuring a reverse proxy; the console and API share
an origin. The POST also accepts JSON media types only, rejecting form and
plain-text content.

### Organization members

Member routes check the caller's membership in the organization from the path.
They are nested under `/api/organizations/{organization_id}`. After
authentication, the middleware in `api/organizations/current.rs` checks CSRF for
POST, DELETE and PATCH, parses the UUID and loads the caller's membership once. It
injects `CurrentOrganization { id, access }` into request extensions. Handlers
extract `Extension<CurrentOrganization>` and use its `id` for their queries;
write handlers call `organization.require_owner()` without another membership
query. DELETE and PATCH extract only the named target `email` from the path.

An absent membership or organization returns `404 organization_not_found`.
Adding, removing and changing member access requires `OWNER`; ordinary members receive
`403 owner_required` for these operations. Both `MEMBER` and `OWNER` can list
members.

### POST /api/organizations/{id}/members

Send `Content-Type: application/json` and `X-WD40-Portal: 1` with:

```json
{ "email": "alice@example.org" }
```

Only `email` is accepted. Surrounding whitespace is trimmed and case is preserved.
The address must have a non-empty local part and domain separated by one `@`,
contain no whitespace or control characters and fit within 254 bytes. No account
lookup, DNS check, invitation or confirmation is performed.

The endpoint inserts the membership directly with `MEMBER` access and a database
creation timestamp. It returns `201` with:

```json
{
  "email": "alice@example.org",
  "access": "MEMBER",
  "created_at": "2026-10-18T12:00:00+00:00"
}
```

An existing membership returns `409 member_already_exists`, including concurrent
duplicate additions. Its access is never overwritten. The membership becomes
usable when the person authenticates with that exact verified email.

### GET /api/organizations/{id}/members

Any member of the organization can retrieve its members. Returns `200` with an
array of the objects above, including `MEMBER` and `OWNER` entries, ordered by
email. The result contains only memberships from the requested organization.

### DELETE /api/organizations/{id}/members/{email}

Requires `OWNER` access and `X-WD40-Portal: 1`. Encode the target email as a URL
path segment, for example `alice%2Bteam%40example.org`. There is no request body.

Returns `204` after removing the membership, or `404 member_not_found` if it does
not exist. Removing an `OWNER` returns `409 owner_removal_forbidden`. Another
owner must first change that member's access to `MEMBER` using PATCH. The deletion
also filters by `MEMBER` access in SQL to avoid removing an owner.

### PATCH /api/organizations/{id}/members/{email}

Requires `OWNER` access, `Content-Type: application/json` and `X-WD40-Portal: 1`.
Encode the target email as a URL path segment, as for DELETE, and send:

```json
{ "access": "OWNER" }
```

Only `access` is accepted, with the exact value `MEMBER` or `OWNER`. An owner can
promote or demote another member in the selected organization. Changing their own
access is forbidden, including requests that keep the current role, and returns
`403 self_update_forbidden`. Identity uses the same exact verified email as
organization membership lookup.

Returns `200` with the updated member object, preserving its email and creation
date. Requesting the existing role also succeeds. A missing target membership
returns `404 member_not_found`; the route never creates a membership.

Organization responses use `Cache-Control: no-store`, including errors. Errors
use `{ "error": "code" }`:

| Status | Code | Meaning |
| --- | --- | --- |
| `400` | `invalid_request` | Malformed JSON, missing fields, wrong type, unknown fields or invalid access value. |
| `400` | `invalid_name` | Trimmed name is empty or exceeds 128 Unicode characters. |
| `400` | `invalid_email` | Email does not meet the address rules above. |
| `400` | `invalid_organization_id` | Organization identifier is not a UUID. |
| `400` | `invalid_path` | Path parameters could not be decoded. |
| `401` | `unauthenticated` | Missing or invalid authentication cookie. |
| `403` | `email_not_verified` | Email is absent, empty or not verified. |
| `403` | `csrf_rejected` | Missing or invalid custom header, or rejected fetch context. |
| `403` | `owner_required` | Caller is a member but not an owner. |
| `403` | `self_update_forbidden` | Caller attempted to change their own member access. |
| `404` | `organization_not_found` | Organization is absent or caller is not a member. |
| `404` | `member_not_found` | Target membership does not exist. |
| `409` | `member_already_exists` | Email is already a member of this organization. |
| `409` | `owner_removal_forbidden` | Target membership has OWNER access. |
| `413` | `payload_too_large` | Request body exceeds 16 KiB. |
| `415` | `unsupported_media_type` | Request content type is not JSON. |
| `500` | `database_error` | Database operation failed; no SQL details are returned. |

Authentication provider errors retain the authentication API's status and code.

## Console architecture

Group console code by feature under `console/src/features/`:

```text
src/
  root.tsx, routes.ts, style.css
  routes/                        Pages and route exports
  layouts/                       Session and console layouts
  features/
    auth/
      session.ts                 OIDC, current user HTTP calls and logout
      queries.ts                 Cached current user and session lookup
      router.ts                  Authentication middleware, guards and loaders
      auth.test.ts
      components/
    organizations/
      api.ts                     Types, validation, queries and mutations
      router.ts                  Organization selection, guards and hooks
      organizations.test.ts
      components/                Creation dialog and organization switcher
      members/
        api.ts                   Member queries, mutations and cache updates
        components/              Add, edit and remove dialogs
  components/
    ui/                          Generic UI components
    shell/                       Console frame and navigation
  lib/
    api.ts                       Shared HTTP transport and AuthError
    query-client.ts              Shared cache and session isolation
    utils.ts
  themes/
```

Add each new feature under `features/<feature>/`. Keep its types, validation,
API operations, cache updates, error messages and components together. A single
`api.ts` is enough initially; split it by operation only when needed. Keep tests
beside their feature. Shared UI belongs in `components/`; shared infrastructure
belongs in `lib/`.

Routes and layouts assemble feature components and expose React Router loaders
and middleware. Feature `router.ts` files own guards and navigation context.
Keep HTTP transport in `lib/api.ts` and feature-specific behavior in its feature.
Use direct imports, such as `~/features/organizations/api.ts`, without adding
controller, service or repository layers or barrel files.

Preserve the shared query client and session-generation checks when adding API
operations so late responses cannot update another session's cache. The session
layout retains the `layouts/session` route ID used by organization hooks.

## Console authentication

`console/src/features/auth/session.ts` owns the browser authentication protocol
and authentication API calls without importing React or React Router. It exports:

- `getLoginUrl(returnTo?, { signal }?)`, which requests an authorization URL and
  saves the exact callback URI and return destination in `sessionStorage`.
- `login(returnTo?)`, which calls `getLoginUrl` and navigates the browser to the
  provider. The default destination is the current page, including query and hash.
- `completeCallback({ code, state, redirectUri })`, the low-level callback POST.
- `getCurrentUser({ signal }?)`, which returns a user or `null` on `401` and
  propagates other failures.
- `logout()`, which clears the server cookies and pending browser login metadata.
  The caller chooses the subsequent navigation, typically to a public route.

`lib/api.ts` provides the shared HTTP transport. Requests use the compiled base
path, same-origin credentials and `no-store`. POSTs, DELETEs and PATCHes include
`X-WD40-Portal: 1`; JSON bodies also set `Content-Type`. The shared `AuthError`
exposes an error `code` and HTTP `status`. Network errors propagate.
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

`root.tsx` exports a `clientMiddleware` from `features/auth/router.ts`. Before route
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

Only the login page is public. The session layout guards the organization chooser
and application routes, sending unauthenticated users to `/login` with their return
destination. The organization layout also checks membership before child loaders
or actions run. Logout clears cookies and the browser query cache before opening `/login`.
`features/auth/router.ts` provides `optionalUser(args)`, `requireUser(args)` and
`withUser(loader?)` for routes that need identity. A fresh React Router context
shares a lazy user promise during each navigation or fetcher call, backed by
TanStack Query across navigations. There is no persistent browser cache or
additional React authentication provider.

`features/auth/queries.ts` defines the `current-user` query, keyed by the console base path,
and `getSessionUser()` for the guards. `getCurrentUser()` remains the uncached HTTP
operation. The cached result is a `CurrentUser` or `null` and stays fresh for
60 seconds. Navigations reuse fresh data and share concurrent requests; the first
navigation after expiration waits for a new check. The query owns its abort signal,
so cancelling one navigation does not cancel another navigation's request. There
is no polling, focus/reconnect refetch or automatic retry for this query.

An identity change discards private queries and mutations while preserving the
completed user query. Successful OIDC callbacks clear the previous session before
the next lookup. Successful logout cancels reads, discards private data and caches
`null`, so the login page needs no immediate `/api/me` request. Cache-level handlers
apply the same expiration to `401` errors from protected queries and mutations.
HTTP errors carry the local request's session generation so late failures cannot
expire a newer session; login and callback failures do not expire the user cache.
The session layout observes cached user changes without fetching and revalidates
its guards before showing protected content for a different user or after logout.
Network/provider failures remain errors rather than an anonymous result. Rust
continues to authenticate protected API requests regardless of the browser cache.

For a route with an optional user:

```ts
import type { LoaderFunctionArgs } from "react-router";
import { optionalUser } from "~/features/auth/router.ts";

export async function clientLoader(args: LoaderFunctionArgs) {
  return { user: await optionalUser(args) };
}

export function HydrateFallback() { return null; }
```

For a protected route:

```ts
import { withUser } from "~/features/auth/router.ts";

export const clientLoader = withUser(async ({ user, request }) => {
  // Load this route's data here, after authentication has completed.
  return { user };
});

export function HydrateFallback() { return null; }
```

`withUser()` without an argument returns `{ user }`. The wrapper runs during
initial hydration and only calls the supplied loader after obtaining a user.
Without a user it redirects to `/login`. Provider outages and other errors
propagate instead of triggering a login. Layout middleware runs before child
loaders and actions; a parent loader alone cannot guard parallel child loaders.
Client actions can also call `requireUser(args)` before their mutations.
Rust remains responsible for API access control.

Components can consume the result through `loaderData` or `useLoaderData` when
the UI is added. No authenticated content or user lookup runs during prerendering.
Any route used for a document callback must be served by Rust. Recognized dynamic
organization routes use the embedded SPA fallback, including when mounted under
a deployment prefix.

### Organization state

`lib/query-client.ts` owns one TanStack Query client shared by router middleware and
React's `QueryClientProvider`. `features/organizations/api.ts` defines the API calls, response
types and shared query options. Query keys include the console base path, issuer,
subject, email and email verification state. Lists stay fresh for 60 seconds;
concurrent consumers share the same request. There is no polling or persistent
browser storage. Stale lists can refresh on navigation, window focus or reconnect.
Authentication failures are not retried automatically. Logout or an identity
change cancels private queries and drops their results while preserving the
corresponding authenticated or anonymous user result.

`features/organizations/router.ts` checks the URL's organization before child loaders run.
An ID missing from a cached list triggers one refresh before access is refused.
Components use `useOrganizations()` and `useOrganization()` to read that same cache;
the URL determines the active organization. The session loader revalidates on
navigation so components follow any change of authenticated identity.

The chooser redirects automatically when exactly one organization is available.
An empty list offers creation and manual refresh, and explains that a proprietor
can add the user's verified email. Multiple organizations require an explicit
selection. Unavailable organizations return to the chooser with a message.
The left navigation switches organizations above the workspace links and offers
the same creation dialog, including in the mobile navigation drawer.
Creation updates the shared cache from the POST response and opens the new
organization without an immediate GET. Results from a previous session are ignored.
### Organization members in the console

`/{organization_id}/members` uses the existing table, dialog and confirmation
components. Members can search the list locally by email and see access levels
and creation dates. Owners can add an email with `MEMBER` access, remove an
ordinary member after confirmation, or change another member's role. The role
dialog shows the current role and offers `MEMBER` and `OWNER`; saving is disabled
until the selection changes and while the request is pending. The edit action is
hidden on the caller's own row and for ordinary members. Owners must be demoted
by another owner before they can be removed.
Adding a member grants access directly; no invitation email is sent.

`features/organizations/members/api.ts` defines the API calls and TanStack Query options. The
query key includes the authenticated identity and organization ID, with a
60-second freshness window and no polling. Successful mutations update the
matching cached list after cancelling older reads; results from previous sessions
are ignored. The sidebar observes the cached count without fetching members on
other pages. The page handles loading, empty searches, request errors and refresh.
Permission errors refresh the caller's organizations; an expired session returns
to login and a lost organization membership returns to the chooser. The Rust
fallback serves direct `/members` loads under the configured deployment prefix.

The focused integration check is available from `portal/console/` with
`node --test src/features/auth/auth.test.ts src/features/organizations/organizations.test.ts`.
Node.js strips the TypeScript types directly.
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
