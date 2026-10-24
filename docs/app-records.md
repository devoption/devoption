# Experimental app-scoped append-only records

This opt-in implementation under [#306](https://github.com/devoption/devoption/issues/306)
and [#308](https://github.com/devoption/devoption/issues/308) provides a bounded
host capability for one trusted app. The default `app-worker` remains stateless.
The managed broker can bind an operator-installed grant to a saved release and
deployment review. It does not provision PostgreSQL, identities, credentials or
schema, and does not authorize activation on an existing installation. Remote
managed databases remain outside this slice under #293.

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
because a response was lost. No read, list, update, delete, SQL, connection or
collection-selection operation is available to the guest.

This interface is independent of an app's HTTP route and response schema. Apps
must validate request method, media type, origin, content and abuse policy before
calling it; the host independently enforces its configured field schema. Apps
must not report success before the host confirms the write. App authentication,
public intake rate limiting, retention and an administrator interface are not
implemented by this slice.

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

PostgreSQL access is fixed parameterized INSERT and host-only replay comparison,
scoped by the configured app/collection. The host never receives guest SQL. Each
append opens one peer-authenticated **Unix socket** connection using explicit
host/database/user configuration; it does not inherit PostgreSQL environment,
passwords or a connection string. There is no TCP fallback or remote plaintext
transport. A managed remote database with verified TLS and credential rotation
requires a separate implementation. The guest retains denied files, environment,
sockets, DNS and outbound HTTP, and receives no credentials or database errors.

Apply [records.sql](../crates/devoption-runtime/src/records.sql) with an operator
migration identity to a dedicated app database, never platform owner/catalog
storage. Production role provisioning is outside this slice. A runtime role
needs only schema USAGE and table INSERT/SELECT; the guest cannot issue SELECT.
Separate app databases/roles remain the intended operator isolation boundary;
app/collection predicates also prevent a grant from mixing namespaces in a
shared proof database. The proof uses its temporary cluster's existing OS user,
not production role provisioning. No automatic migration runs in the worker.

The database table must be permanent/logged and `fsync` and `full_page_writes`
must be on. Each transaction explicitly sets synchronous commit on, a 1000 ms
statement timeout and a 500 ms lock timeout. Success follows COMMIT. A 500 ms
connection timeout and two-second whole-operation deadline bound host I/O.
Cancellation drops the client and aborts the connection driver;
an interrupted COMMIT can still have persisted, hence retry IDs. One append is
allowed per request. Existing serial guest admission and five-second request
deadline remain. No pool, retry loop or persistent guest state is introduced.
These guarantees assume a correctly administered PostgreSQL server and reliable
storage; they do not prove backups, replication or disaster recovery.

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
python3 scripts/check_app_records.py --pg-bin /PATH/TO/POSTGRESQL/bin \
  --worker target/debug/devoption-runtime \
  --component examples/record-app/target/wasm32-wasip2/release/devoption_record_fixture.wasm
```

The fixture uses `/append/UUID` and a generic label/note schema. The script proves
guest-to-row writes, normalization/replay/conflict, host validation, unknown
import refusal without a grant, a lost HTTP acknowledgement, bounded database
lock failure, app namespace separation, database outage, refusal with fsync off,
and persistence after worker restart and PostgreSQL crash-stop/recovery. With
the record grant enabled, it also probes denied files/environment/TCP/UDP/DNS
and outgoing HTTP to loopback, private, metadata and public destinations.
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
