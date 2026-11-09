# Durable HTTPS deliveries

The first [#296](https://github.com/devoption/devoption/issues/296) contract is a
host-owned JSON relay: a trusted app commits a bounded delivery job to its own
PostgreSQL database, and a separate explicitly enabled operator process sends it
to one reviewed HTTPS integration. HTTP requests do not execute the delivery.
This implements persisted jobs, scheduling, leases, retries and observations
without Redis or an arbitrary background Wasm/shell execution interface.

The default stateless worker and existing records/HTTPS grants enable no jobs.
Enqueue authority and dispatcher activation are separate actions. Stopping the
app's HTTP worker does not stop accepted jobs; stop/cancel the delivery worker
separately. An operator can supervise it using the supplied optional systemd
template. The platform does not install, start or automatically recover that unit.

## Admission and authority

Add an explicit `jobs` object to the protected [record grant](app-records.md):

```json
{
  "project": "0123456789abcdef0123456789abcdef",
  "environment": "preview",
  "secret": "SERVICE_API_KEY",
  "revision": 1,
  "endpoint": "https://api.example.test/events",
  "receiver_dedup_seconds": 172800
}
```

This is a value-free policy example. Project must match the grant's app ID.
Endpoint/name/revision rules match [scoped HTTPS](app-https.md); the receiver's
reviewed deduplication retention must be at least 48 hours. That field records an
operator-verified receiver contract, not a capability that makes an arbitrary
provider deduplicate. The operator must check it before enabling the dispatcher.

Promotion retains policy version 3 with this policy, explicit read permission and
the complete grant digest. Versions 1 and 2 remain unchanged. Deployment review
shows the queued endpoint/secret revision and receiver horizon. Changing the grant
invalidates prior reviews/recovery through the existing grant/fingerprint checks.
Old brokers/runtimes do not support version 3; update coordinated runtime, broker,
server and UI before enabling it. No existing app-record table migration runs.

The authoritative [WIT](../crates/devoption-runtime/wit/records.wit) adds
`devoption:app/delivery-jobs@0.1.0`:

```text
enqueue(id, payload, delay-seconds) -> accepted | replayed | error
```

IDs are canonical lowercase UUIDs. Payload is one JSON object, at most 24 KiB
both as supplied and canonically serialized. Duplicate object keys at any depth,
invalid JSON and delays outside 0–3600 seconds return `invalid`. The host owns
project, endpoint, credential selection, database, retry policy and time.
No guest destination, SQL, command, executable or secret value is accepted.
The existing one-database-operation-per-request budget includes enqueue.

`accepted` follows an acknowledged synchronous PostgreSQL COMMIT. Repeating the
same app/ID, semantic JSON value, initial delay and exact policy returns `replayed`
without changing its schedule or terminal state. A changed value/delay/policy
under that ID returns `conflict`. Unavailable/uncertain commit returns `unavailable`;
retry the same identity and values. Neither acceptance nor replay confirms an
external side effect. Records append plus enqueue are separate transactions;
this profile supplies no atomic business-write/outbox operation.

Apply [jobs.sql](../crates/devoption-runtime/src/jobs.sql) using a separately
authorized migration identity in a dedicated app database. It creates a new
optional schema; workers never apply migrations or alter existing data. Use
protected peer-authenticated Unix sockets and separate dedicated service roles:

- Enqueuer: schema USAGE and table INSERT/SELECT only.
- Dispatcher/control: schema USAGE, SELECT, and UPDATE only on `state`,
  `attempts`, `generation`, `lease_until`, `due_at` and `outcome`.
- Neither runtime role owns the table or receives DDL, DELETE or platform-store
  access. The dispatcher receives no INSERT or payload/identity update grant.

The enqueuer's existing grant supplies its connection identity. The dispatcher's
separate grant uses its own role but the same app and `jobs` policy. App/database
selection cannot come from a guest. This is a single-node local PostgreSQL path;
remote database credentials/TLS and hostile multi-tenant database hosting remain
outside the contract.

## Dispatcher and secret custody

The explicit foreground command is:

```sh
/verified/path/devoption-runtime job-worker /protected/dispatcher.json /run/credentials/selected-unit/https.json
```

The second file uses the existing host HTTPS credential packet: `policy` contains
project/environment/secret/revision/endpoint, `addresses` contains reviewed pinned
public IPv4 addresses, and `value` contains the selected bearer value. Root/operator
custody must establish the exact value and revision before delivery. This
standalone profile does not automatically resolve the platform's encrypted
project secret store or inherit a controller's credentials. The operator-supplied
revision is trusted metadata; operators must increment it when replacing a value.

Credentials belong in protected runtime-only files, supplied with systemd
`LoadCredential` or equivalent private delivery. They stay out of source, builds,
arguments, environment, job rows, release metadata, browser responses and logs.
The process snapshots and hashes the exact parsed bytes; a live file change or
removal stops further iterations. A running systemd credential copy is independent
of its source: stop the dispatcher and revoke the provider credential when immediate
revocation is needed. Root and the explicitly approved receiver remain trusted.

The dispatcher matches all policy fields before claiming. It uses the same
host HTTPS client as direct integrations: verified TLS, fixed canonical HTTPS
endpoint, pinned public addresses, no ambient proxy, no redirects, no Cookie or
platform credentials, one POST per attempt and a three-second transport bound.
The host adds the bearer value and derives `Idempotency-Key` from app ID plus
operation UUID. Attempt numbers, worker identities and restarts never enter the key.
Provider responses are bounded and discarded after status classification; raw
responses/errors never enter observations. The guest receives no credential or
provider response through enqueue.

The [systemd template](../examples/systemd/devoption-delivery@.service) is an
uninstalled operator example. Substitute the verified executable path and a
dedicated non-login user/group; configure exact `IPAddressAllow` entries matching
the reviewed packet. The default denies all IP traffic. The template limits one
process group to one CPU, 256 MiB and 16 tasks, with a 12-second stop allowance,
read-only system files, no home/platform store and restricted address families.
Its configuration and resource/network enforcement need installation-specific
verification. The foreground CLI itself starts no unit, identity, child or listener.
After a reboot, the exact approved private credential must be supplied before
starting the unit; a missing runtime-only credential blocks service startup and
leaves jobs durable in PostgreSQL. Queue backups contain no credential value.
No provisioning or production activation is authorized by these instructions.

## Leases, retries and recovery

Admission serializes each app's capacity check with insertion. The ledger retains
at most 10,000 jobs per app, including terminal/deduplication records, and accepts
at most 20 new operations per rolling minute. Exact retries remain usable at the
limit. Database unavailability, unsafe durability settings and capacity exhaustion
fail visibly without deleting rows. PostgreSQL storage/WAL/backup sizing remains
an operator responsibility.

A short transaction commits a 30-second lease with incremented generation before
delivery. One live lease per app is allowed across competing dispatchers. Expired
leases can be reclaimed after a process/host restart. The host rechecks fencing
before POST; completion updates only the current unexpired lease generation.
An old worker cannot overwrite a newer outcome. A process suspended after its
last check can still race with cancellation/expiry, so receiver deduplication is
essential for external safety.

There are at most five attempts. Transport/ambiguous errors, HTTP 408/429 and
server errors retry after 5, 30, 120 and 600 seconds; 2xx records `succeeded`, and
other 4xx records terminal `failed`. Redirects are refused by the HTTPS layer.
Exhausted uncertain delivery, changed policy, invalid stored payload or a job
older than 24 hours becomes `needs_review`. Missing/inconclusive results never
become success. Backoff and due/deadline comparisons use database time, requiring
a correctly administered clock.

SIGTERM/SIGINT stops further claiming and lets the bounded current attempt finish.
A forced kill preserves unfinished leases. Each database stage has the existing
500 ms connection, 1000 ms statement, 500 ms lock and two-second overall bounds;
delivery has three seconds. Stored JSON rendering is bounded to 64 KiB before
transfer and must reconstruct a valid at-most-24-KiB canonical object. A worker
failure or outage cannot silently discard accepted rows.

The receiver must atomically commit operation key, payload digest, result and side
effect. A matching retry returns the retained result; a changed payload conflicts.
That contract yields one receiver-side effect across lost replies, duplicate
delivery and supported backup replay. Generic HTTP success alone cannot establish
this property for a provider. Keep receiver deduplication state for the complete
reviewed recovery horizon. Jobs restored after their 24-hour deadline require
reconciliation and cannot automatically replay against an expired receiver ledger.

## Human observations and cancellation

An authorized operator using the dispatcher role can run:

```sh
/verified/path/devoption-runtime jobs /protected/dispatcher.json status
/verified/path/devoption-runtime jobs /protected/dispatcher.json cancel UUID
```

Status includes exact state counts, current policy digest and up to 100 pending or
exceptional IDs with attempts, timestamps, lease, policy digest and bounded outcome
codes. Payloads, bearer values, database details and raw errors are omitted.
Unavailable observations exit unsuccessfully; they never report an empty queue.
An HTTP app's health is independent of queue/delivery health. Operators must
monitor these observations and the separate service before accepting this profile.

Cancelling queued/retry-wait work records `cancelled`. Cancelling a lease records
`needs_review` with `cancel_ambiguous`, increments fencing and retains the operation
identity. It cannot retract a POST already accepted by the receiver. The response
reports whether a matching nonterminal row changed. A cancelled/failed/review job
is never resurrected by enqueue replay. No automatic terminal deletion, reschedule
or manual retry-with-new-identity API is provided. Reconciliation must establish
the receiver's actual outcome before any separate corrective action.

Back up jobs and application data with PostgreSQL-aware procedures; preserve
operation IDs and receiver deduplication state across restoration. Keep the
receiver ledger authoritative when replaying an older queue backup. Capacity
exhaustion requires an operator decision, not silently resetting the ledger.
The jobs reference saved JSON and policy, not an executable release: subsequent
app code cannot rewrite a pending job or replace its selected integration.

## Disposable validation

```sh
cargo build -p devoption-runtime --release --locked --offline
cargo build --manifest-path examples/record-app/Cargo.toml --release --target wasm32-wasip2 --locked --offline --features delivery-jobs --target-dir /tmp/devoption-job-proof
python3 scripts/check_app_jobs.py --pg-bin /PATH/TO/POSTGRESQL/bin \
  --worker target/release/devoption-runtime \
  --component /tmp/devoption-job-proof/wasm32-wasip2/release/devoption_record_fixture.wasm
```

The proof uses a private disposable peer-authenticated database, distinct minimal
enqueue/dispatcher database roles, a real compiled guest, separate killed/restarted
dispatcher processes and a local TLS receiver with a transactional SQLite effect
ledger. It kills delivery after receiver commit, then verifies one effect after
lease recovery and after restoring an older queue backup. It tests delayed work,
clean/ambiguous cancellation, exact retry/conflict, recursive duplicate-key refusal,
five-attempt exhaustion, live credential/policy change refusal, database outages,
unsafe fsync refusal, competing claims, generation fencing and retained capacity.
The receiver uses a synthetic token; no real recipient/provider is contacted and
no system trust is installed. Loopback/custom CA injection exists only in the
ignored test executable; production loading has no such options.

CI runs this proof against PostgreSQL 16 and checksum-pinned 18.6, alongside the
existing direct HTTPS TLS/endpoint/header/size/redirect/timeout denial proof and
stateless/record/app deployment acceptance. The tests move timestamps in their
disposable database to exercise lease/backoff/deadline boundaries; they do not
claim a 24-hour elapsed observation or a real provider's retention contract.
