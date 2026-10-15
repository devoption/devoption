# In-memory build approval lifecycle

[#192](https://github.com/devoption/devoption/issues/192) implements
`devoption_builds::approvals`, a bounded bookkeeping prerequisite of
[#17](https://github.com/devoption/devoption/issues/17). It records an owner action,
expires it after five minutes, and consumes or revokes it once. No production
caller, HTTP route, UI, persisted approval, executor or runner-event issuer exists.
No real workload is approved by these APIs or their temporary-fixture tests.

The [execution trust contract](execution-trust.md) remains authoritative for the
complete review and external verification requirements. This model enforces the
in-memory lifecycle only; it does not establish that an owner actually reviewed
anything or that supplied metadata is true.

## Binding and trusted caller

`Approvals::approve` accepts an `OwnerId`, a queued `BuildJob` and a SHA-256 review
context digest. It retains the owner, JobId, complete immutable BuildPlan value,
digest and a monotonic issuance time. `OwnerId` wraps the server-assigned identity
bytes; it is not an account, password, session or proof of permission. The build
crate does not depend on server authentication types. No credential is generated.

The digest is a required binding to external review context not fully represented
by BuildPlan: verified toolchain/executable identity, fresh catalog/source binding,
verified dependency contents, execution account, owned path identities, effective
environment/configuration, network policy, resource caps, output/retention policy
and policy version. The future adapter must present and verify this complete
record and use a deterministic unambiguous encoding before hashing it. No such
encoding or verifier is implemented here. A caller-selected hash, constant digest
or BuildPlan equality alone cannot prove verification or confer execution authority.

Before recording approval, the adapter must authenticate the explicit owner action
through the protected mutation boundary and bind it to exactly the displayed record.
Before consumption, it must freshly verify all external state, compute the current
context digest, and serialize admission with job cancellation and state updates.
It must revoke approval on failed verification instead of passing stale metadata.
The model cannot detect a file change, failed external check, or cancellation made
to some other copy of a job. Rust field privacy is not a security boundary against
an untrusted caller in the process.

## Lifecycle

| Operation | Result |
| --- | --- |
| Approve a new queued attempt | Pending record and opaque handle |
| Consume matching owner, job, full plan and context before expiry | Consumed |
| Consume with changed plan/context or nonqueued job | Invalidated |
| Revoke pending approval | Revoked |
| First observation at or after five minutes | Expired |
| Reuse a terminal record | Rejected; no renewal or reversal |
| Wrong owner, attempt or registry | Rejected without consuming the rightful record |

Handles and registries may be cloned, but share the same locked records. The clock
is sampled after lock acquisition, including on consumption; waiting cannot extend
expiry. Exactly one competing consume/revoke operation can succeed. Terminal
transitions drop the retained plan/context and keep a small tombstone. Public errors
are fixed and omit owners, paths, environment values and digests. Handles and review
records have no Debug/serialization interface.

Consumption changes only approval bookkeeping. It returns no execution capability,
leaves the BuildJob queued and cannot construct a RunnerEvent. It is not a durable
queue admission transaction. An eventual runner adapter must connect successful
consumption to one admission; any admission failure needs a fresh attempt and review.
Revoking an already consumed approval fails and cannot cancel a running process.
Session expiry does not retrospectively undo a consumed approval.

At most 64 attempt records may be created during one registry's lifetime, including
terminal tombstones. Duplicate JobIds are rejected even after expiry/revocation;
retries require a new JobId. At capacity the registry fails closed. It never evicts
records or silently replaces itself. This is a bounded prerequisite, not an
indefinitely operating production approval store. Production integration needs a
separately reviewed durable attempt-ID and retirement strategy before lifting this
limit. Pending expiry is observed lazily on issue/status/consume/revoke, with no
background timer; retained data remains bounded when idle.

A handle contains a weak reference to its exact issuing registry. A new registry
cannot accept old handles, even with identical owner/job/plan/context values; dropped
registries cannot be resurrected through a handle. A service restart therefore loses
all usable approvals. Retained clones are the same registry, not a restart. Global
JobId uniqueness and interrupted job recovery remain the future store's duty; creating
a new registry does not establish either. Never restore an approval from a saved
job state or automatically resume work.

## Validation and remaining integration

Tests cover exact five-minute expiry, every BuildPlan field group and external
context changes, owner/attempt mismatch, replay, revocation, queued cancellation,
registry replacement, shared concurrent consumers/revokers, lifetime capacity and
poisoned-lock failure. A compile-fail test rejects fabricated handles. Synthetic
plans reuse existing disposable metadata fixtures; no build or child is launched.

Run `cargo test -p devoption-builds --locked --offline`, then the
[workspace checks](development.md#checks). These tests do not establish live HTTP
approval, filesystem freshness, dependency verification, durable atomic admission,
OS isolation, process cleanup or restart recovery. Those remain #17 integration
work under the trust contract. [Job lifecycle records](build-jobs.md) still have
no production runner-event issuer; consuming an approval does not bypass that seal.
