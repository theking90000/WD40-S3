# portal-db

SeaORM database crate for portal data. SQLite is enabled by default; enable
`postgres` for PostgreSQL, with `default-features = false` for PostgreSQL only.
Both drivers can be enabled together. UUID and Chrono type integrations are
enabled for organization identifiers and creation timestamps.

The package is named `portal-db` to avoid workspace name conflicts and stays in
`portal/db/`. Its Rust crate name is `portal_db`.

The caller opens the connection with `portal_db::Database::connect`, optionally
using `portal_db::ConnectOptions`, and owns its shutdown. No connection is opened
implicitly.

The standalone portal opens the database selected by `WD40_PORTAL_DATABASE_URL`,
applies migrations before serving HTTP and closes the connection on shutdown.
Its default URL is `sqlite://portal.db?mode=rwc`, creating `portal.db` in the
working directory when absent.

Library callers apply migrations explicitly with `Migrator::up(&db, None).await?`,
importing `portal_db::{Migrator, MigratorTrait}`, then pass a connection clone to
`portal::Portal::new(config, db.clone()).await?`. They close their retained
connection after requests have drained. The portal API exposes the connection
to its handlers through Axum state.

## Organizations

`portal_db::Organization` is the persisted model; `portal_db::organization`
exposes its SeaORM entity and active model. The `organizations` table contains
`id: Uuid`, `name: String` and `created_at: DateTimeWithTimeZone`, all required.

Inserting through `organization::ActiveModel::insert` or `save` generates a UUID
v4 when `id` is unset. Random-source failures return a database error. Bulk or
direct entity inserts must supply identifiers themselves. The database fills
an omitted `created_at` with `CURRENT_TIMESTAMP`; updates leave it unchanged
unless explicitly set. Names are globally unique and case-sensitive. The portal
trims surrounding whitespace before insertion. The unique-name migration fails
if existing rows contain duplicate names; it does not rename or delete them.

## Organization members

`portal_db::OrganizationMember` is the persisted model;
`portal_db::organization_member` exposes its SeaORM entity and active model.
The `organization_members` table contains `organization_id: Uuid`,
`email: String`, `access: OrganizationMemberAccess` and
`created_at: DateTimeWithTimeZone`, all required.

The primary key is `(organization_id, email)`. An email can belong to multiple
organizations, once per organization. Emails are stored as supplied. A separate
index on `email` supports listing memberships across organizations.
`OrganizationMemberAccess::Member` and `OrganizationMemberAccess::Owner` are
stored as `MEMBER` and `OWNER`. A database check constraint rejects other values;
inserts must supply access explicitly. The database fills an omitted
`created_at` with `CURRENT_TIMESTAMP`.

SeaORM relations connect each member to its organization and each organization
to its members. The foreign key references `organizations.id` with
`ON DELETE CASCADE`, so deleting an organization removes its memberships.

## Buckets

`portal_db::Bucket` is the persisted model; `portal_db::bucket` exposes its
SeaORM entity and active model. The `buckets` table contains `name: String`,
`organization_id: Uuid`, `created_at: DateTimeWithTimeZone` and
`access: BucketAccess`, all required. The name is the primary key and is unique
across all organizations. The database fills an omitted `created_at` with
`CURRENT_TIMESTAMP`. An index on `organization_id` supports organization lists.
SeaORM relations connect buckets and organizations; the foreign key uses
`ON DELETE CASCADE`.

Access is stored as text with a database check constraint:

- `PRIVATE`: authentication is required for every S3 operation.
- `PUBLIC`: GET and HEAD object require no key; every other operation requires
  authentication.
- `PUBLIC_LIST`: GET and HEAD object, and LIST objects in the bucket, require no
  key; every other operation requires authentication.

These values describe S3 access, not access to the portal management API. This
change stores the policy; enforcement in the S3 access layer remains separate.

The database does not validate bucket name syntax. The creation handler checks
the general-purpose AWS naming rules without trimming or lowercasing the input.
It rejects the reserved `-an` suffix because account-regional AWS namespaces are
not represented here. Periods are allowed; Transfer Acceleration is not enabled.

The handlers and local router are in `portal/src/api/organizations/buckets/`.
They support creation, listing and access updates, with owner checks for writes
and organization-scoped queries. The bucket router is mounted under
`/api/organizations/{organization_id}` with the existing organization membership
middleware. Bucket names,
organization identifiers and creation timestamps cannot be changed by these
handlers.

## Credentials

`portal_db::Credential` is the persisted model; `portal_db::credential` exposes
its SeaORM entity and active model. The `credentials` table contains
`key_id: String`, `organization_id: Uuid`, `access_type: CredentialAccessType`
and `created_at: DateTimeWithTimeZone`, all required. The key ID is the primary
key and is unique across all organizations. The database fills an omitted
creation timestamp with `CURRENT_TIMESTAMP`. An index on `organization_id`
supports organization lists, and the organization foreign key uses
`ON DELETE CASCADE`.

Access types are stored as text with a database check constraint:

- `ALL` describes access to every bucket of the organization, including future
  buckets.
- `RESTRICTED` describes access only to explicitly selected buckets. The join
  table and bucket selection are deferred; without associations, this access
  type must grant no access when S3 enforcement is connected.

The portal generates 20-byte ASCII alphanumeric key IDs and 40-byte ASCII
alphanumeric secrets with `getrandom`. Secrets appear only in the creation
response. They are not stored in this database or the WD40 credentials registry.
This tranche manages metadata and generation; these credentials do not yet
authenticate S3 requests. Deletion removes portal metadata only.

## Code structure

Add the following modules as the schema grows; do not create empty placeholders:

```text
src/
├── lib.rs                 # Module declarations and public exports
├── entities/
│   ├── mod.rs
│   └── organization.rs    # One entity file per table
└── migrations/
    ├── mod.rs             # Migrator and ordered migration list
    └── m..._create_organizations.rs
```

- Keep persistence models, columns and relations in `entities/`. HTTP handlers
  and request/response types belong in `portal/src/api/<feature>/`.
- Add versioned migrations with the first schema. Keep applied migrations
  unchanged and append new ones for schema changes. Prefer SeaQuery and portable
  types when supporting both SQLite and PostgreSQL.
- Keep operation-specific queries with their portal feature; extract shared
  queries within that feature when needed. Do not add generic repository or
  service layers around SeaORM.
- Keep modules private unless callers need them, and expose the required types
  through `lib.rs`. Connection setup, migration execution and shutdown remain
  the caller's responsibility.
- Store portal-owned data here; reuse WD40 registries for the data they already own.
