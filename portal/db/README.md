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
`portal::Portal::new(config, db.clone(), credentials, policies).await?`, together
with already opened S3 credentials and policy registry handles. They close their
retained connection after requests have drained. The portal API exposes the
connection to its handlers through Axum state.

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
- `RESTRICTED` describes access only to buckets selected through
  `credential_buckets`. An empty selection grants no access when S3 enforcement
  is connected.

`portal_db::CredentialBucket` and `portal_db::credential_bucket` expose that
join table. Its required fields are `organization_id: Uuid`, `key_id: String`
and `bucket_name: String`, with primary key `(key_id, bucket_name)`. Composite
foreign keys reference `credentials(organization_id, key_id)` and
`buckets(organization_id, name)`, backed by unique indexes on those parent
columns. This enforces that both records belong to the same organization.
Both foreign keys use `ON DELETE CASCADE`; removing a credential or bucket
removes its associations. Recreating a bucket with the same name does not
restore associations.

SQLite connections must retain `PRAGMA foreign_keys = ON`, which the SQLx
driver enables by default. Creation and access updates validate the selection
and write the credential and associations in one transaction. Switching to
`ALL` clears the selection; switching to `RESTRICTED` or updating its selection
requires an explicit list. Existing restricted credentials start with an empty
selection.

The portal generates 20-byte ASCII alphanumeric key IDs and 40-byte ASCII
alphanumeric secrets with `getrandom`. Secrets appear only in the creation
response. They are not stored in this database or the WD40 credentials registry.
This tranche manages metadata and generation; these credentials do not yet
authenticate S3 requests. Deletion removes portal metadata only.

## Synchronization state

`portal_db::BucketSync` and `portal_db::bucket_sync` expose the `bucket_sync`
table. Its key is `bucket_name`; `public_policy_id` and `private_policy_id` are
stable UUIDs for the two WD40 policies. Each column is unique, and the two IDs
must differ within a record. `portal_db::CredentialSync` and
`portal_db::credential_sync` expose `credential_sync`, keyed by `key_id`, with a
unique UUID `principal_id`. Convert these UUIDs with `as_u128()` when calling
WD40. No secret is stored in either table.

Inserting through either entity's active model generates unset identifiers with
`getrandom`. Random-source failures return `DbErr`; direct and bulk inserts must
supply their own IDs. Updates retain identifiers unless explicitly replaced.

Both tables contain `generation: i64`, `applied_generation: i64`,
`last_error: Option<String>` and `delete_requested: bool`. Database defaults are
generation 1, applied generation 0, no error and no deletion request. Constraints
require a positive desired generation and an applied generation between zero and
the desired generation. A fresh record therefore represents pending work.
Application is complete when the generations match, no error remains and no
deletion is requested. Generations belong to the portal and are independent of
the policy registry's global revision.

Explicit synchronization records `sync_incomplete` before contacting WD40,
replaces it with an error code on failure and clears it on successful application.
Acknowledgements check the generation and stable identifiers to avoid confirming
a newer desired state. A cancelled operation leaves its incomplete marker.

These tables have no foreign keys to business entities, so their identifiers
survive resource or organization deletion for later WD40 cleanup. The caller
must retain these records until cleanup succeeds and prevent reuse of their
resource names or key IDs while cleanup is pending.

Migration `000008` creates the tables without backfilling existing resources.
Current HTTP handlers do not create or update synchronization records yet.
No synchronization is triggered by this schema.

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
