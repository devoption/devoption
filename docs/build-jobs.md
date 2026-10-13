# Build-job lifecycle records

[#127](https://github.com/devoption/devoption/issues/127) adds the pure Unix
`devoption_builds::jobs` domain. It uses the existing immutable
[`BuildPlan`](build-plans.md) without duplicating its fields. There is no process
launch, runtime caller, route, UI, storage, approval or production event issuer.
No job records are created in the real instance. Application builds remain unavailable.

## Identity and records

`BuildJob::queued` owns a plan and an attempt `JobId`. Events must match both the
ID and the complete plan value, including dependencies, source, toolchain,
environment, paths, target and proposed limits. This equality is an in-memory
identity check, not a stable serialized plan digest or authorization. It does not
recheck external state or authenticate source selection. A future store must
allocate unique IDs, reject duplicates, and give every retry a new ID. The current
model has no store and does not enforce uniqueness across separately created records.

The plan and state have read-only accessors. `State::Succeeded` carries
`ArtifactMetadata` for the plan's single binary: a SHA-256 claim and nonzero byte
count. Other states cannot carry an artifact. Metadata construction neither hashes
nor opens a file; even an accepted size is not an enforced resource limit. No
artifact path, release, deployment or independent provenance claim is introduced.
Failure categories are fixed; records contain no unbounded logs or arbitrary errors.

## Transition rules

| Current state | Allowed next state | Required input |
| --- | --- | --- |
| Queued | Running | Matching sealed runner event |
| Queued | Failed (preparation only) | Matching sealed runner event |
| Queued | Cancelled | `cancel_queued()` or matching sealed runner event |
| Running | Succeeded with artifact, Failed, Cancelled | Matching sealed runner event |
| Succeeded, Failed, Cancelled | None | Terminal records cannot be rewritten |

All other transitions, repeated events and mismatched identities fail without
changing the record. Queued jobs cannot skip directly to success. Cancelling a
running job requires a future runner's confirmation that its processes have
stopped and cleanup is complete; `cancel_queued()` rejects running jobs. No running
cancellation request transport is implemented. The first accepted terminal event
wins; later completion/cancellation events fail. This is a sequential in-memory
rule, not a concurrency, durable-delivery or crash-recovery guarantee.

## Why a status cannot assert success

Public state and metadata values are descriptions, not trusted results. Only
`RunnerEvent` can drive runner transitions, and its fields are private with no
public constructor, deserializer or production issuer. Module-local tests alone
create synthetic events. Compile-fail tests preserve this public boundary and
prevent direct state assignment. Safe production callers can currently create and
cancel a queued record, but cannot mark a job running or successful.

A future trusted issuer must be separately implemented and reviewed. Before
issuing any event it must bind the actual attempt, exact approved plan and process
state; terminal events must follow child cleanup. Success must additionally require
a successful process exit, selection of the expected binary, and hashing/size checks
of its actual bytes. Metadata alone, test fixtures, a client status assignment or
a restored record cannot satisfy that boundary. Rust field privacy is not a sandbox,
cryptographic attestation or protection against malicious/unsafe code in the process.

The tests reuse the existing temporary catalog fixture, remove its synthetic Git
directories before plan validation, and exercise deterministic state transitions
without starting processes. The full transition matrix, plan/attempt mismatches,
queued cancellation versus a late start, terminal replay and nonempty artifact
metadata are covered. Synthetic successful records make no real-build claim.

Persisted records, restart recovery, real approval/runner integration, cancellation
delivery, bounded logs, artifact retention and execution limits remain future work.
The [disposable runner profile](disposable-runner-proposal.md) received separate
approval for an initial temporary fixture attempt and one diagnostic repeat.
The [initial result](disposable-build-rehearsal.md) and [repeat result](disposable-build-repeat.md)
record successful compiles followed by rejected artifacts; neither issued a job
event or grants permission for another attempt or production execution.
The separate [Linux fixture proof](offline-fixture-build.md) remains test-only and
does not issue these events or authorize a production runner.
