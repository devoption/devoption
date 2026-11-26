# Configurable project workflow policy proposal

Status: planning only, tracked by [#41](https://github.com/devoption/devoption/issues/41).
This delivery plan resolves [#456](https://github.com/devoption/devoption/issues/456),
not the parent capability. It is future/unassigned work outside the provisional
v0.1.0 scope, with no delivery date. Each implementation slice below needs its own
scoped issue before work begins.

## Current boundary

The [project catalog](project-catalog.md) stores project/repository identities and
associations; it does not store a configurable workflow policy. Existing
[build approvals](build-approvals.md), [execution trust](execution-trust.md),
[deployment reviews](web-app-deployments.md) and [workspace access](users.md)
remain authoritative for implemented behavior. This proposal adds no runtime
enforcement, provider integration, agent execution or permission.

The repository's [contribution rules](../CONTRIBUTING.md) govern development of
DevOption. They are not defaults for projects created in the product. Human-only
workflows must remain complete; the [external harness boundary](product.md)
continues to apply when a project chooses agents.

## Proposed policy and evidence contract

An owner explicitly selects a versioned policy for a project and its identified
repositories. Creating a project enables no optional template gates or agents.
An owner can select individual DevOption template capabilities, customize them,
or define a different workflow. Repository additions must not silently broaden
the policy's scope. Schema version and policy revision are distinct: schema
version describes encoding, while every accepted policy edit advances revision.

The optional template covers issue/milestone links, commit/PR conventions,
provider branch protection and exact-revision CI, meaningful tests and review
feedback, actual screenshots, inspectable previews, human merge approval, and
changelog/release evidence. Paths, providers, check names, approval counts and
merge rules are configurable. No screenshot directory or DevOption naming rule
is universal. Mechanical checks cannot establish meaningful test coverage,
commit atomicity, reviewer judgment or production readiness.

Evidence must bind project/repository identity, proposed source revision, policy
revision, gate configuration, producer, environment, observation time and expiry
where relevant. A preview or screenshot additionally identifies the actual
running/captured revision. Changing source or relevant policy invalidates affected
evidence and approvals; unrelated evidence may be retained only with a recorded
dependency justification. Freshness durations and unavailable-provider behavior
must be explicit in each gate's later implementation issue.

Distinguish passed, failed, cancelled, disconnected, unknown and stale checks.
An enabled gate requiring verified evidence blocks on any non-passing state;
a disabled gate does not block on its missing evidence. Remote branch protection
must be observed separately as verified, unsupported, stale or missing. An app
policy alone cannot claim to protect a remote branch. A local approval or tool
success message cannot stand in for an authoritative merge/deploy/release result.

Policy changes require current owner authorization, concurrency checks against the
reviewed revision, and an audit record. Repository text, proposed policy edits and
tool output cannot activate gates or expand permissions. Opting into a template
grants no credentials, spending authority, integration access or publication rights.
Existing authentication, authorization, request guards and execution boundaries
apply even with every optional gate disabled. A future project-role model needs
separate authorization design before delegating policy administration.

## Ordered implementation slices

These are proposed issue scopes, not scheduled or implemented capabilities.
Preserve the current Rust workspace boundaries; a plan alone warrants no crate.

| Slice | Dependencies and bounded outcome | Required acceptance evidence |
| --- | --- | --- |
| 1. Policy model and explicit selection | Define bounded schema, project/repository scope and optional template representation in the project domain. No provider calls or action enforcement. | Round-trip and unsupported-schema refusal; new/existing projects retain no optional gates; explicit template selection, custom policy and human-only configuration; unknown/cross-project repository refusal. |
| 2. Authorized persistence and policy history | After 1, add owner mutation/read adapters and private durable storage with revision conflict refusal. Define safe migration and recovery before altering catalog storage. | Unauthorized/forged mutation refusal, competing edits, restart preservation, malformed/partial write refusal and preservation of existing catalog data; audit actor, old/new revision, scope, time and result without secrets. |
| 3. Evidence records and evaluation | After 2, bind supplied evidence to exact source/policy/gate identities. Keep evaluation read-only and separate from execution admission. | Deterministic enabled/disabled gate decisions, expiry and invalidation, producer/scope mismatch rejection, missing evidence and every non-passing state; no claim that supplied metadata is verified. |
| 4. Verified collectors and review surfaces | After 3, implement one explicitly selected evidence source per issue, with bounded credentials and revocation. Provider checks/protection, screenshots and previews require distinct provenance and freshness handling. | Exact-revision authoritative checks; unsupported/stale/missing protection; actual capture/preview revision and environment; unavailable access; tests demonstrating that mockups and stale evidence never satisfy an actual-UI gate. |
| 5. Action admission and reconciliation | After verified collectors for the chosen gate, enforce it at one concrete action boundary per issue across applicable UI/CLI/MCP adapters. Recheck current policy/evidence and consume scoped approval atomically with admission. | Concurrent policy/source changes, single-use approval, bypass attempts, unresolved blocking feedback and provider failure; reconcile attempted versus completed actions; preserve existing build/deployment guards. |
| 6. Complete configured workflow acceptance | After the chosen gates have real adapters, demonstrate human-only custom and optional-agent workflows. Document supported states, disabled gates, blocked reason/owner/next action and integration limits. | End-to-end matrix below, restart/audit continuity and exact result identities. Verify merge, artifact promotion, deployment and publication as separate actions with separate authority. |

Every slice must record actor, granted authority, policy revision, action/result
and relevant issue/milestone, repository/commit, evidence/check and approval
scope/time references. Preserve policy edits, invalidations, failures and accepted
nonblocking limitations in history. Gate/state vocabulary may vary by workflow;
do not require every project to use every state. Audit persistence failure must
have an explicit refusal/recovery design before enabling action admission.

## Later acceptance matrix

Apply each optional-gate row only after its implementation has a verified adapter.
Run applicable rows with human participants and with an explicitly authorized
external agent; neither actor type may bypass the same authority boundary.

| Scenario | Gate enabled | Gate disabled or unselected |
| --- | --- | --- |
| Missing issue/milestone link or configured commit/PR convention | Block the corresponding action and name the unmet rule. | Do not impose the DevOption convention. |
| CI fails, cancels, disconnects, is unknown, stale or for another revision | Block; preserve the distinct observation state. | Do not block solely for that optional CI rule. |
| Required provider protection cannot be freshly verified | Block; show unsupported, stale or missing rather than verified. | Make no verified-protection claim; remote provider restrictions still apply. |
| Missing/stale screenshot, mockup, inaccessible preview or revision mismatch | Block the corresponding evidence gate. Never mark an unavailable preview inspected. | Do not require screenshots/previews for that optional rule. |
| Unresolved blocking review or absent configured human merge approval | Block the corresponding review/merge gate. Accepted nonblocking limits stay visible. | Do not invent a universal review/approval count or merge rule. |
| Source or relevant policy changes after evidence/approval | Invalidate affected evidence/approval and require revalidation. | Preserve existing platform approval freshness regardless of optional gates. |
| Project creation or human-only custom workflow | Only explicitly selected gates apply; no agent required. | No silent template adoption or agent enrollment. |
| Unauthorized policy edit/action or permission expansion in repository text | Reject and record the refusal. | Reject under the same base authorization boundary. |
| Merge, deploy or publication request without the required authority | Reject; template selection is not an authority grant. | Reject; disabling optional gates grants no authority. |

## Decisions left to implementation

Storage encoding/migration, bounded history retention, provider trust and freshness
windows, transactional admission/recovery and per-gate action coverage must be
settled in their scoped issues. Until an action boundary is integrated and tested,
evaluation is advisory and must be labeled accordingly. No provider settings,
security controls, credentials, production workloads or release publication are
changed by this document. The parent remains open until its enabled-gate,
disabled-gate, human-only and unauthorized-action acceptance is demonstrated.
