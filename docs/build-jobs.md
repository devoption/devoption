# Build-job lifecycle records

[#127](https://github.com/devoption/devoption/issues/127) adds the pure Unix
`devoption_builds::jobs` domain. It uses the existing immutable
[`BuildPlan`](build-plans.md) without duplicating its fields. There is no process
launch, runtime caller, route, UI, storage or production event issuer.
The separate [approval lifecycle](build-approvals.md) records single-use in-memory
review actions; it is not integrated with job admission or runner transitions.
No job records are created in the real instance. Application builds remain unavailable.

## Identity and records

`BuildJob::queued` owns a plan and an attempt `JobId`. Events must match both the
ID and the complete plan value, including dependencies, source, toolchain,
environment, paths, target, proposed limits and the retained complete source
snapshot, including owned file bytes and the root Cargo proof. A different verified
source revision or source bytes cannot start or complete the original
attempt. This equality is an in-memory
identity check, not a stable serialized plan digest or authorization. It does not
recheck external state or authenticate source selection. The model does not enforce
uniqueness across separately created records. The
separate [history store](build-history.md) allocates random attempt IDs, rejects
collisions with retained history and persists Queued before returning each fresh
attempt. Its update API rejects unknown IDs. It is not a global identity registry
or an execution-admission transaction.

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
| Queued, Running | Interrupted | `interrupt()` records lost supervisor continuity |
| Succeeded, Failed, Cancelled, Interrupted | None | Terminal records cannot be rewritten |

All other transitions, repeated events and mismatched identities fail without
changing the record. Queued jobs cannot skip directly to success. Cancelling a
running job requires a future runner's confirmation that its processes have
stopped and cleanup is complete; `cancel_queued()` rejects running jobs. No running
cancellation request transport is implemented. The first accepted terminal event
wins; later completion/cancellation events fail. This is a sequential in-memory
rule, not a concurrency, durable-delivery or crash-recovery guarantee.

## Interruption and recovery boundary

Under [#208](https://github.com/devoption/devoption/issues/208), `interrupt()` moves
only Queued or Running records to terminal Interrupted. This records loss of
supervisor continuity, not confirmed cancellation, child termination or cleanup.
It preserves the exact plan and attempt ID, carries no artifact and rejects every
later runner event, including late success or cancellation. Existing terminal
outcomes cannot be overwritten. Retrying requires a fresh attempt and review.

`BuildJob::recover_interrupted(id, plan)` creates only an Interrupted record.
It accepts no persisted status or artifact and cannot recreate running state,
approval authority or successful execution. It is a recovery boundary for future
storage, not a storage implementation. A future loader must validate persisted
identity/plan data and use this path for unfinished queued/running attempts;
it must not use `queued()` to silently resume them. Completed-history restoration
needs its own validated, non-authoritative representation and artifact checks.

Interruption does not automatically notify an approval registry. A future adapter
must revoke pending approval as part of its serialized interruption handling.
Consuming against an interrupted record fails as NotQueued and permanently
invalidates that approval; a new registry also refuses to approve that record.
Creating a new registry after restart invalidates old handles, but this model alone
does not prevent callers constructing separate queued records with reused IDs.
Use the history store allocator for new persisted attempts; its guarantees and
cross-store limits are described in the [history guide](build-history.md).

An interrupted label cannot release a runner slot or justify deleting job storage.
Surviving children/writers must be reconciled and cleanup verified before admitting
another job. If that cannot be established, admission must remain blocked.
The lifecycle API adds no persistence, startup hook, process inspection or automatic
recovery. The separate [history store](build-history.md) provides explicit saved-summary
recovery without wiring a startup flow.

## Why a status cannot assert success

Public state and metadata values are descriptions, not trusted results. Only
`RunnerEvent` can drive runner transitions, and its fields are private with no
public constructor, deserializer or production issuer. Module-local tests alone
create synthetic events. Compile-fail tests preserve this public boundary and
prevent direct state assignment. Safe production callers can create/cancel a queued record or mark an unfinished
record interrupted, but cannot mark a job running or successful.

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

The separate [history store](build-history.md) now persists bounded summaries and
explicitly marks unfinished history interrupted. Startup recovery integration,
real approval/runner integration, cancellation delivery, durable logs, artifact
retention and execution limits remain future work.
The separate [bounded output collector](build-output.md) retains caller-supplied
bytes and renders escaped text, but has no runner readers, persistence or job integration.
[Supervision decisions](build-supervision.md) combine time/output limits and
cancellation with a termination grace; they neither signal processes nor issue job events.
The [disposable runner profile](disposable-runner-proposal.md) received separate
approval for an initial temporary fixture attempt and one diagnostic repeat.
The [initial result](disposable-build-rehearsal.md) and [repeat result](disposable-build-repeat.md)
record successful compiles followed by rejected artifacts. These fixture attempts
issued no job events and grant no permission for further builds or production execution.
The separate [Linux fixture proof](offline-fixture-build.md) remains test-only and
does not issue these events or authorize a production runner.
The [rehearsal artifact handoff](artifact-handoff.md) has disposable filesystem
tests but no supervisor integration or event issuer. Its hash receipt alone
cannot complete a job or prove that a runner was stopped.

## Remaining delivery for issue #17

The domain helpers are implemented, but there is no production end-to-end build.
The server/CLI do not instantiate the approval, job, output or supervision models.
These are the remaining integration groups, not claims that the acceptance rows
are complete:

| Area | Existing evidence | Still required |
| --- | --- | --- |
| Review and admission | [Plans](build-plans.md), [typed contexts](review-context.md), [approval lifecycle](build-approvals.md) | Present the complete verified record in an authenticated review UI; approve/cancel routes; fresh catalog/source/context verification; serialize single-use consumption with cancellation and single-job admission |
| Prepared inputs | [Complete source capture](source-snapshots.md), [directory](reviewed-directories.md) and [executable](reviewed-executables.md) checks | Fresh owned work/source/dependency materialization outside checkouts; verified dependency archives/bundle and complete toolchain/configuration manifests; enforce trusted paths, ACL/mount and read-only layout requirements |
| Runner and cleanup | [Output collector](build-output.md), [supervision decisions](build-supervision.md), [real leaf-process tests](process-supervision-fixtures.md), separate [fixture evidence](offline-fixture-build.md) | Select and implement the supported runner/target profile; enforce unprivileged identity, clean environment, network/filesystem isolation and CPU/memory/process/disk caps; launch one job; deliver signals, reap all children and verify cleanup through handoff |
| Durable history and UI | In-memory lifecycle, [private summary storage and explicit recovery](build-history.md), escaped bounded output | Startup recovery integration, leftover reconciliation, admission using allocated attempt IDs, durable logs and retention; project-page states/logs and cancellation feedback |
| Artifact and provenance | Source/toolchain/command metadata; separate [handoff fixture](artifact-handoff.md) | Connect successful exit and verified cleanup to bounded collection of only the reviewed artifact; record durable provenance; expose independently staged verified bytes only for successful jobs |
| Closure tests | Unit/rejection tests and separately approved offline fixtures | Integrated successful HTTP-service build, nonzero exit, timeout, cancellation, stale review, truncation/escaping, restart, artifact rejection and preservation of the original repository |

Do not close #17 based on these helpers alone. Process execution is outside the
scoped lifecycle/helper issues. The runner requires a separately selected
implementation scope and must satisfy the
[trust contract](execution-trust.md); no privileged setup or deployment is implied.
Release promotion (#18) and service deployment/supervision (#19) remain outside #17.
