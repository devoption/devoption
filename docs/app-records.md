# App-scoped PostgreSQL records

This opt-in implementation under [#306](https://github.com/devoption/devoption/issues/306)
and [#308](https://github.com/devoption/devoption/issues/308), extended under
[#293](https://github.com/devoption/devoption/issues/293), provides a bounded
host capability for trusted apps using operator-managed **single-node PostgreSQL**.
This is the supported first database path: host-mediated operations over protected
peer-authenticated Unix sockets. The default `app-worker` remains stateless.
The managed broker can bind an operator-installed grant to a saved release and
deployment review. It does not provision PostgreSQL, identities, credentials or
schema, and does not authorize activation on an existing installation. Remote
database services, native socket drivers and arbitrary SQL are unsupported. A
future remote path needs explicit verified TLS/server identity and credential
custody; this local path never falls back to remote plaintext connectivity.

## Guest contract

The authoritative [WIT](../crates/devoption-runtime/wit/records.wit) defines
`devoption:app/records@0.1.0`. `append(id, payload)` takes a lowercase canonical
UUID retry ID and a JSON object of strings/null, at most 24 KiB. It returns:

| Result | Meaning |
| --- | --- |
| `created` | New row, PostgreSQL acknowledged synchronous COMMIT |
| `replayed` | Same ID and normalized values already saved; no new row |
| `invalid` | Invalid ID, JSON, unknown/duplicate field, type, length or field constraint |
| `conflict` | Same scoped ID with different values; no overwrite |
| `unavailable` | Database/transport/durability/timeout failure; commit may be ambiguous |
| `denied` | Missing grant or more than one append attempt in a guest request |

An unavailable response is never a success. Retry the same ID **and values** after
an uncertain response. Different object key order is immaterial. Values are
boundary-trimmed; absent/null/empty optional strings normalize to null. Reusing
an ID for changed normalized values conflicts. Do not generate a new ID merely
because a response was lost. The original interface remains write-only. No list,
update, delete, SQL, connection or collection-selection operation is available.

The separate optional `devoption:app/record-lookup@0.1.0` import adds
`get(id) -> result<option<string>, read-error>` only when the operator explicitly
sets `"allow_read": true` in the grant. Legacy grants omit this field and remain
write-only; `false` also keeps lookup unavailable. A component requiring lookup
fails validation/linking without this permission. The old `record-imports` WIT
world is unchanged; `record-access-imports` includes both interfaces.

Both interfaces are linked by the pinned Wasmtime WASIp2 HTTP host; the guest
uses the WIT bindings, not a native PostgreSQL socket driver. The host uses the
workspace's locked `tokio-postgres` 0.7.18 driver. Acceptance builds the guest
against `wasip2` 1.0.1 (`wasi:http/proxy@0.2.4`) with the pinned Rust toolchain.

A lookup takes one canonical lowercase UUID and selects one row using fixed,
parameterized app/collection/ID predicates in a read-only transaction. `some`
contains normalized JSON matching the current field schema; `none` means only
that this exact scoped row is absent. `invalid` rejects malformed IDs, `denied`
rejects missing/read-disabled or already-used request authority, and `unavailable`
reports database, timeout, malformed/out-of-schema or oversized stored-data errors.
Unavailable storage is never presented as an empty result. Reads return at most
32 KiB, checked in PostgreSQL before transferring the text and rechecked against
the host schema; extra fields and changed values are not silently filtered or
normalized. PostgreSQL's JSON rendering/optional-field overhead explains the
larger read bound relative to the 24 KiB input bound.

One database operation total (append, lookup or separately granted
[durable enqueue](durable-deliveries.md)) is permitted per guest
request, including invalid/failed attempts. Apps needing read-after-write use a
later request. The host grant is app/collection scoped, not member scoped: apps
must authenticate readers and enforce their own per-record/member authorization
before calling lookup or returning its result. A UUID alone is not authorization.
The public fixture is disposable transport evidence, not a production data API.

This interface is independent of an app's HTTP route and response schema. Apps
must validate request method, media type, origin, content and abuse policy before
calling it; the host independently enforces its configured field schema. Apps
must not report success before the host confirms the write. App authentication,
public intake rate limiting, retention and an administrator interface remain
application/operator responsibilities. App-owned session transport is described
in [app authentication](app-authentication.md).

## Host grant and database boundary

Only the operator's local command selects the grant file, before compilation and
validation. The worker snapshots it once; guests cannot change it. Example JSON:

```json
{
  "app_id": "0123456789abcdef0123456789abcdef",
  "collection": "notes",
  "socket_dir": "/tmp/records-proof/socket",
  "database": "records_proof",
  "user": "worker",
  "fields": {
    "label": {"required": true, "max_chars": 100},
    "note": {"required": false, "max_chars": 1000, "multiline": true}
  }
}
```

Grant files are at most 32 KiB. App IDs are 32 lowercase hex characters;
collection/field names are ASCII alphanumeric/underscore (1–64 characters), and
database/user names use the same character set (1–63). Socket directory must be
absolute and at most 80 bytes. At most 16 fields, each with a maximum of 1–4000
Unicode scalar values. `required` and `max_chars` are mandatory. Unknown config
keys fail closed. Optional field settings are `multiline`, `email` and `choices`
(at most 32 nonempty exact allowed strings). Single-line fields reject control
characters, even at their edges. Multiline permits CR/LF/tab only. `email` checks
a conservative ASCII mailbox/domain syntax; it does not establish ownership or
deliverability and does not accept internationalized/quoted mailboxes.

`allow_read` is an optional boolean, never a truthy string or number. No change to
the existing SQL schema is required to enable reads. Review existing stored values
and application authorization before granting access to an existing collection.
An optional `jobs` object grants the separate delivery interface and uses release
policy version 3; its schema, role permissions and lifecycle are owned by the
[durable delivery contract](durable-deliveries.md). Ordinary record grants enable
neither jobs nor a dispatcher.

PostgreSQL access uses fixed parameterized INSERT, replay comparison and optionally
exact-ID lookup, scoped by the configured app/collection. The host never receives
guest SQL. Each operation opens one peer-authenticated **Unix socket** connection using explicit
host/database/user configuration; it does not inherit PostgreSQL environment,
passwords or a connection string. There is no TCP fallback or remote plaintext
transport. Server identity comes from the operator-protected local socket path
and database-service ownership; PostgreSQL verifies the connecting OS service
identity through peer authentication. TLS is not used on this local transport.
The guest retains denied files, environment,
sockets, DNS and outbound HTTP, and receives no credentials or database errors.

Apply [records.sql](../crates/devoption-runtime/src/records.sql) with an operator
migration identity to a dedicated app database, never platform owner/catalog
storage. Production role provisioning is outside this slice. A runtime role
needs only schema USAGE and table INSERT/SELECT; the guest cannot issue SQL.
Separate app databases/roles remain the intended operator isolation boundary;
app/collection predicates also prevent a grant from mixing namespaces in a
shared proof database. The proof uses its temporary cluster's existing OS user,
not production role provisioning. No automatic migration runs in the worker.

The database table must be permanent/logged and `fsync` and `full_page_writes`
must be on. Each write transaction explicitly sets synchronous commit on, a 1000 ms
statement timeout and a 500 ms lock timeout. Success follows COMMIT. A 500 ms
connection timeout and two-second whole-operation deadline bound host I/O.
Read transactions use the same statement/lock/connect/whole-operation deadlines
and are explicitly read-only. They select by the existing primary key and do not
take the write quota advisory lock. Cancellation drops the client and aborts the connection driver;
an interrupted COMMIT can still have persisted, hence retry IDs. One append
or lookup is allowed per request. Existing serial guest admission and five-second request
deadline remain. No pool, retry loop or persistent guest state is introduced.
These guarantees assume a correctly administered PostgreSQL server and reliable
storage; they do not prove backups, replication or disaster recovery.

The V1 host also bounds each app/collection to 20 new records per rolling minute
and 10,000 stored records. An advisory transaction lock serializes the budget
check with inserts. Budgets come from PostgreSQL rows, so restarting a worker
does not reset them. Exhaustion returns unavailable without a write or deletion;
exact replays still succeed. Operators must monitor capacity and configure
appropriate abuse/retention policy before public use. These fixed first-profile
limits do not provide per-visitor fairness or replace application abuse controls.

## Managed broker installation boundary

After separately authorized database/schema/service-identity provisioning, the
operator may add this table to root-owned `/etc/devoption/deployment.toml`:

```toml
[records]
grant_file = "/etc/devoption/app-records.json"
```

The grant and every ancestor must be canonical, root-owned and protected from
other-user writes. Mode 0600 is appropriate for this private configuration.
It contains no password: this first path uses PostgreSQL peer authentication
with a database role matching the existing OS service identity. Use a dedicated
non-login service account with UID at least 100, distinct from root and the
platform owner, a dedicated primary group and no supplementary groups. Its
PostgreSQL runtime role must be separate from the migration/owner role and have
only CONNECT/schema USAGE/table INSERT+SELECT. Never grant it table ownership,
DDL, UPDATE, DELETE or platform database access.

The socket directory must exist, be canonical and traversable by the service
identity; neither platform nor app identity may own or write it or its ancestors.
No ancestor may be world-writable. Use the database service's protected directory,
not a caller-controlled temporary directory. No socket credential or database
network port is exposed to the guest.

Promotion for the matching project snapshots the exact grant into root-private
release storage and includes its SHA-256 and service UID/GID in the immutable
manifest. New releases from other projects and saved stateless releases receive
no grant. Raw grant bytes and resolved service identity also enter the installed
runtime fingerprint. Review displays the storage permission and exact grant
digest before approval. Changed/revoked grants or identities require a newly
promoted release and a fresh review; old reviews/restart intent cannot restore
that access. Restoring a saved **stateless** release remains possible through a
fresh review and never adds the records import.

Read-enabled releases retain policy version 2 with explicit `allow_read=true`;
write-only releases preserve the exact version-1 policy shape. The saved grant
digest and declared read authority must agree. Deployment review states whether
the release can append only or also look up records; granting or removing reads
invalidates old reviews/recovery intent. Older brokers do not understand version
2 and must not be used to activate read-enabled releases. Keep the coordinated
runtime, broker, server and UI together. Retained version-1 releases remain readable
by the new broker and cannot silently gain read permission.

There is no password to rotate in this profile. Rotate/revoke access by changing
the protected database role/socket/grant and promoting/reviewing a new release;
stop the old app unit when immediate revocation is needed. Database peer-role
revocation prevents new operations because no connection pool survives requests.
Changes to a grant file alone do not mutate the running worker's snapshot.

The broker retains existing memory/CPU/task, read-only filesystem and network
limits. It substitutes the dedicated service UID/GID for DynamicUser, adds the
host's AF_UNIX transport, and mounts only immutable component/grant files and
the configured database socket directory read-only. Validation links the import
and validates schema without connecting or executing the guest. It keeps its
private network and omits the socket mount. A running guest still denies all
standard files/environment/socket/DNS/outgoing-HTTP operations.

A running worker snapshots its grant; editing/removing the file alone does not
instantly revoke that worker. An operator revoking access must stop the owned app
unit as well. Restart reconciliation refuses changed policy and leaves stopped
intent. Health checks establish guest HTTP health, not database availability;
verify a real app write before declaring a storage-enabled deployment accepted.
Database failure returns unavailable from append while the app may still serve
static pages and health responses.

Back up both PostgreSQL data (using PostgreSQL-aware backup/restore) and platform
configuration/immutable releases before installation. Do not copy a live data
directory as a database backup. Restoring a component does not roll back or erase
its rows. Keep schema/data backward compatible with saved releases; production
backup scheduling, retention, credential rotation and disaster recovery require
operator configuration and are not automatically installed here. Preserve
database data during rollback; never drop it to recover a prior stateless app.

## Disposable local proof

Use existing PostgreSQL binaries, a non-root OS account and the pinned Rust
toolchain with `wasm32-wasip2` installed. The script creates a mode-0700 temporary
cluster/socket directory, uses peer authentication, binds no TCP listener, starts
no host service and creates no password. It stops/waits its processes and removes
only its temporary directory.

```sh
cargo build -p devoption-runtime --locked --offline
cargo build --manifest-path examples/record-app/Cargo.toml --target wasm32-wasip2 --release --locked --offline
cargo build --manifest-path examples/record-app/Cargo.toml --target wasm32-wasip2 --release --locked --offline --features record-reads --target-dir /tmp/devoption-record-read-proof
python3 scripts/check_app_records.py --pg-bin /PATH/TO/POSTGRESQL/bin \
  --worker target/debug/devoption-runtime \
  --component examples/record-app/target/wasm32-wasip2/release/devoption_record_fixture.wasm \
  --read-component /tmp/devoption-record-read-proof/wasm32-wasip2/release/devoption_record_fixture.wasm
```

The fixture uses `/append/UUID` and a generic label/note schema. The script proves
guest-to-row writes, normalization/replay/conflict, host validation, unknown
import refusal without a grant, a lost HTTP acknowledgement, bounded database
lock failure, app namespace separation, database outage, refusal with fsync off,
and persistence after worker restart and PostgreSQL crash-stop/recovery. It also
checks persistent rate/storage quotas and an exact row/timestamp digest after
pg_dump/pg_restore into a separate disposable database. With
the record grant enabled, it also probes denied files/environment/TCP/UDP/DNS
and outgoing HTTP to loopback, private, metadata and public destinations.
The optional read component exercises `/record/UUID` through the actual WASIp2
host after a write-only-to-read-enabled component change. It proves the default
grant cannot link lookup, reads across worker/database restarts and restored
backups, app namespace isolation, missing/invalid IDs, server-side byte/schema
bounds, wrong database/service identity, outages and lock timeout refusal. Its
feature flag selects a trusted test component, not broader platform Cargo feature
admission. The JSON report states whether read acceptance was actually enabled.
The explicit worker command is:

```sh
target/debug/devoption-runtime records-worker COMPONENT PORT serve GRANT.json
```

`validate` checks imports with the supplied grant without contacting PostgreSQL
or executing guest code. The old `app-worker ... validate` rejects components
importing records. These local commands do not provide the broker's Linux
service confinement. Do not treat them as production installation instructions.

`scripts/check_managed_records.py --disposable-system-host` additionally exercises
the real installed broker and systemd profile on a fresh Linux runner, including
peer-authenticated writes through a dedicated non-login runtime role, guest
denials, restart recovery, changed-grant refusal and return to a saved stateless
release. It refuses existing deployment files, reserved units/identities and its
reserved fixture root. It temporarily creates only its own test identities,
database and files, uses no passwords, and cleans them up after stopping its units.
See CI for exact built fixture arguments. Never run it on a workstation or live
installation.
With `--read-component`, this managed proof promotes and approves a different
compiled component with an explicit version-2 read grant, reads the earlier
release's row, reconciles the worker and reads it again without rewriting data.

Linux CI runs the generic proof against both checksum-pinned PostgreSQL 18.6 and
the Ubuntu 24.04 runner's PostgreSQL 16. It runs the managed systemd proof against
the latter without starting or changing the runner's preinstalled database service.
