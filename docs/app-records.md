# Experimental app-scoped append-only records

This is a local opt-in implementation slice under [#306](https://github.com/devoption/devoption/issues/306)
and [#293](https://github.com/devoption/devoption/issues/293), **not an installed
platform deployment capability**. The normal `app-worker`, release manifest,
deployment broker, runtime fingerprint and systemd profile remain stateless.
Do not install this worker path on an existing platform or grant it database
access until a subsequent reviewed integration binds the grant and service policy
to deployment approval. This command is for trusted disposable local proof only.

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
