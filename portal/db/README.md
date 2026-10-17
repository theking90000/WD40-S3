# portal-db

SeaORM database crate for portal data. SQLite is enabled by default; enable
`postgres` for PostgreSQL, with `default-features = false` for PostgreSQL only.
Both drivers can be enabled together. Optional SeaORM type integrations are
disabled until needed.

The package is named `portal-db` to avoid workspace name conflicts and stays in
`portal/db/`. Its Rust crate name is `portal_db`.

The caller opens the connection with `portal_db::Database::connect`, optionally
using `portal_db::ConnectOptions`, and owns its shutdown. No connection is opened
implicitly.

Add persistence entities under `src/entities/` and versioned migrations under
`src/migrations/` with the first schema. Keep HTTP request/response types in
portal and reuse WD40 registries for the data they already own. Use portable
types and migrations when supporting both database engines.

This crate does not yet define tables or migrations and is not wired into portal.
