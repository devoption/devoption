# Proposed durable app jobs

Status: **proposal for [#296](https://github.com/devoption/devoption/issues/296)**,
not an accepted runtime contract or implemented capability. Review this design
before adding a job interface, schema or executable mode. No queue, dispatcher,
background execution or live integration is enabled by this document.

## Current boundary and proposed first slice

The [HTTP worker](web-app-deployments.md#enforced-runtime-policy) creates a fresh
bounded guest for each request. [App records](app-records.md) permit an optional
durable append, but expose no guest read/update/claim operation.
[Scoped HTTPS](app-https.md) permits one bounded POST per request with an exact
approved secret revision and destination. It supplies neither a durable queue
nor a retry loop. An HTTP handler that spawns a future or sleeps cannot establish
durable delayed work under this contract.

Propose one opt-in, platform-owned dispatcher for one trusted app, using a
dedicated PostgreSQL job schema and a separately validated Wasm job export. Reuse
the existing PostgreSQL transport and host-mediated HTTPS enforcement; do not
introduce Redis, a second queue product, arbitrary shell commands or external
agent scheduling. The ordinary stateless HTTP profile remains the default.

The first supported job performs one JSON POST to one reviewed HTTPS endpoint.
It is eligible only where the receiver implements durable idempotency for the
entire delivery/reconciliation horizon. This is at-least-once execution with
receiver-side deduplication; no universal exactly-once side-effect promise is
possible across a PostgreSQL transaction and an unrelated HTTPS service.

## Admission and durable identity

Propose a versioned host interface, separate from the existing records ABI. It
accepts an app-scoped operation UUID, normalized JSON payload and a due time.
The host fixes the project, job kind, destination and approved release identity;
the guest cannot choose a command, executable path, credential or another app.
Payloads are bounded to 24 KiB and due times to now through 24 hours in the future.
Do not place secrets in payloads.

An operator installs the schema and dedicated peer-authenticated roles in the
app's database; the platform owner database is never a job store. A host-only
enqueue role has only its fixed admission procedure; a separate dispatcher role
may claim/update its app's job rows. Guests receive neither SQL nor database
credentials, sockets or a general read API. PostgreSQL-aware backup/restore must
include job rows and deduplication identities together with application data.

The admission transaction inserts a unique `(app_id, operation_id)` row and
commits synchronously before acknowledging accepted work. An exact duplicate
returns its existing job identity; changed payload, due time, job kind or release
under the same ID conflicts. A lost acknowledgement must be retried with the
same ID and values. Database outage/timeouts return unavailable and never an
invented acceptance. Database time controls due/lease comparisons.

For workflows that require a business write and a job together, the new fixed
host operation must commit both atomically in the same app database. Calling the
current append interface and a later enqueue separately is insufficient: a crash
between those calls can lose the follow-up. A general application transaction API
is outside the first slice.

## Claims, execution and recovery

Persist `queued`, `leased`, `retry_wait`, `succeeded`, `failed`, `cancelled` and
`needs_review`, with creation/due times, attempt count, lease generation/deadline,
release/grant digests and bounded outcome codes. Payloads remain private; public
observations contain identities and bounded summaries only.

A claim uses a short PostgreSQL transaction with row locking and a generation
increment. Commit the lease before executing outside that transaction. Expired
leases become retry-eligible after restart. Completion is a conditional update
on the current lease generation; an old worker cannot complete a newer claim.
Losing the lease prevents further host capability calls by that invocation.
It cannot retract a network request already accepted by the receiver.

The job guest runs in a separate bounded worker process, never in the HTTP
request's Store, browser session or controller event loop. Propose a versioned
`devoption:app/jobs` world with an explicit run export and a bounded result;
ABI/import compatibility must be validated before promotion. Reuse the existing
guest file/environment/socket/DNS denial and host resource policy. HTTP health
and latency must not depend on a long-running job.

Initial proposed limits are one executing job per app, four enabled job apps per
node, 1,000 nonterminal jobs per app, five attempts per job and a five-second
invocation deadline. Reuse the existing 64 MiB guest-memory and fuel limits.
Lease duration is 30 seconds, with no extension by the guest. Failed transient
attempts retry after 5, 30, 120 and 600 seconds; terminal rejection, changed policy
or exhausted attempts never silently retry. A job still unresolved after 24 hours
enters `needs_review`. These are proposed policy bounds, not measured capacity.

On shutdown, stop claiming, allow admitted invocations up to five seconds, then
cancel/stop the owned job workers. Durable unfinished leases remain for recovery;
do not mark interrupted work successful. Stop/cancellation is rechecked before
each external capability call. An already-dispatched POST may still complete.
Cancellation of ambiguous delivery becomes `needs_review`, retaining the stable
operation identity rather than claiming the side effect was prevented.

## HTTPS authority and duplicate delivery

Bind enqueue and dispatch to an immutable saved release and its exact reviewed
grant. Before each attempt, revalidate project association, enabled running
intent, component bytes, runtime fingerprint, job policy, destination and exact
secret revision. A missing/revoked/changed grant blocks execution for human
review. Restart or a later deployment cannot silently substitute new code,
destination or credential into an existing job. Retain referenced releases until
their jobs resolve; build/release retention must treat these as live references.

Reuse the [HTTPS grant](app-https.md) restrictions: one fixed canonical endpoint,
verified TLS, validated and pinned public addresses, denied unrelated/private
destinations, host-injected bearer value, no ambient proxies/redirects and bounded
POST/response sizes. The job export may make at most one such call per attempt.
The host derives a stable `Idempotency-Key` from app and operation identities and
binds it to the exact payload; retry attempt numbers never enter that key.

The test receiver durably stores key, payload digest and result with its side
effect atomically. Matching retries return the prior result; mismatched payloads
conflict. Losing the response after receiver commit must not create a second
effect. The receiver's retention period must exceed the maximum retry, manual
recovery and supported backup-restore replay horizon. If that contract expires
or is unknown, ambiguous jobs require reconciliation instead of automatic replay.
Generic HTTP success alone does not prove an arbitrary provider deduplicates.

## Human visibility and storage pressure

Propose owner-only observations of pending/due counts, current lease, attempts,
last bounded error category, selected code/grant identity and terminal outcome.
Sanitize logs and never expose bearer tokens, payloads or raw upstream errors.
Distinguish unavailable status from an empty queue. Manual retry requires fresh
authority and the original idempotency identity; it cannot erase a prior result.

Keep compact completed-operation identities and payload digests for the receiver's
supported replay horizon. Bound active payload storage separately. Exhausted
storage refuses new jobs visibly; silently deleting deduplication identities to
admit work is unsafe. Select measurable retention/disk bounds and the supported
backup horizon in implementation before offering unlimited job throughput.

## Required implementation evidence

No runtime evidence is supplied by this proposal. A future implementation must
use disposable database/worker processes and a local TLS receiver, not a real
recipient or vendor. The fixture-only resolver/CA must remain unavailable to
production grants, following the existing scoped HTTPS proof.

| Fault or scenario | Required observation |
| --- | --- |
| Crash before/after enqueue COMMIT or lose acceptance response | No accepted job disappears; retry keeps one identity |
| Simultaneous duplicate admission or competing claims | One row and one current lease; changed payload conflicts |
| Kill dispatcher, app or database; restart host | Durable due jobs recover, with no execution from stopped intent |
| Receiver commits, reply is lost, then worker dies | Retry uses the same key; exactly one receiver-side effect |
| Lease expires while an old worker is still alive | Old generation cannot complete or start another POST |
| DNS/TLS/redirect/private-destination/secret-policy faults | Denial without credential leakage or alternate destination |
| Change release or revoke/rotate credentials before retry | Block for review; no automatic policy substitution |
| Limits, poisoned storage, exhausted retries or database outage | Bounded resource use and visible truthful failures |
| Cancel or shut down during claim, execution or ambiguous POST | Durable state and honest side-effect uncertainty |
| Backup restore after prior delivery or deduplication expiry | Safe same-key replay or manual reconciliation |
| Ordinary stateless app and concurrent HTTP traffic | Unchanged grants and bounded independent HTTP handling |

Split implementation into reviewed persistence/admission, worker/claim lifecycle,
HTTPS custody/deduplication proof, then owner observations and restart/restore
acceptance. Coordinate with [#293](https://github.com/devoption/devoption/issues/293)
for service grants and [#314](https://github.com/devoption/devoption/issues/314)
for release/build references. Passing unit tests of a state machine alone cannot
satisfy #296's actual restart and external-side-effect acceptance criteria.
