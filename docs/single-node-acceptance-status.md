# Single-node acceptance reconciliation

This reconciles #19, #20, #131, #230, #233 and #234 against main
`34630827cb22fad811b42f1a32ce886fa430089d`, including merged
[PR #255](https://github.com/devoption/devoption/pull/255). It records the selected
trusted WASIp2 profile and existing evidence; it starts no workload and does not
prepare or approve a product release. The task is
[#256](https://github.com/devoption/devoption/issues/256).

| Issue | Disposition | Evidence / remaining scope |
| --- | --- | --- |
| [#230](https://github.com/devoption/devoption/issues/230) | Completed | Fresh source-built Ubuntu install, canonical public HTTPS origin, dedicated service/storage, owner login/project creation, denied anonymous/cross-origin access and reboot in the [cloud report](public-cloud-acceptance.md) |
| [#233](https://github.com/devoption/devoption/issues/233) | Completed | Two explicit hostnames on one IPv4, public certificate issuance and accelerated renewal, update/rollback/reboot, host/control separation and private backends in the [cloud report](public-cloud-acceptance.md); local certificate/upstream error cases in [hostname evidence](app-hostnames.md#observed-ubuntu-rehearsal) |
| [#234](https://github.com/devoption/devoption/issues/234) | Completed | All six owner-facing steps, A/B builds/deployments, failure/recovery, running/stopped reboots, staged restore and data-preserving reinstall in the [cloud report](public-cloud-acceptance.md), using the [operator runbook](first-web-app.md) |
| [#20](https://github.com/devoption/devoption/issues/20) | Completed for the selected profile | Criterion mapping below; source-only Ubuntu support and bounded same-node recovery remain explicit |
| [#131](https://github.com/devoption/devoption/issues/131) | Superseded; closed as not planned | The native diagnostic repeat still rejected its artifact. The selected Wasm build path has later accepted artifacts; no native retry is required or claimed |
| [#19](https://github.com/devoption/devoption/issues/19) | Open, narrowed to remaining evidence | Selected runtime is implemented; real occupied-port and invocation/cleanup identity integration cases remain unproven, as detailed below |

## Installation and recovery: #20

| Original criterion | Existing implementation and validation |
| --- | --- |
| Clean Ubuntu source installation with pinned inputs | [Cloud source compilation and provenance](public-cloud-acceptance.md#environment-and-source-installation); [installation prerequisites and acquisition](public-linux-install.md#supported-shape-and-prerequisites) |
| Minimal HTTP fixture and complete reviewed workflow | Public `examples/web-app`, the accepted [WASIp2 contract](decisions/0004-first-wasm-web-app.md), and the [operator walkthrough](first-web-app.md); real browser approvals and A/B results in the cloud report |
| Storage, bounds, permissions and cleanup ownership | [Build policy](web-app-builds.md#enforced-policy-and-recovery), [deployment policy](web-app-deployments.md#enforced-runtime-policy), [snapshot rules](same-node-recovery.md) and cloud removal/reinstall inventories |
| Backup/restore, restart and previous-release recovery | Snapshot create/verify/stage/adoption, saved-A recovery and running/stopped reboots passed in the cloud run; [snapshot tests](../scripts/test_recovery_snapshot.py) cover corruption, unsupported state and non-overwrite behavior |
| Honest supported-platform and distribution limits | Source-only Ubuntu x86_64, one trusted stateless app and same-installation restore; no installer, published package, cross-version migration or portable/disk-loss recovery claim |
| Repeatable disposable integration validation | [First-app runbook](first-web-app.md) replayed on clean local and fresh cloud nodes; real manual review/approval gates plus [external routing checks](../scripts/check_public_app.py), repository fixtures and snapshot tests. This is an operator-led integration procedure, not a one-command unattended lifecycle test |

The source-compilation gap recorded before PR #255 is resolved. The integration
criterion is satisfied by the repeatable operator-led procedure and its identified
automated checks; closing #20 does not claim a fully automated provision/build/
deploy/recover harness. Cloud setup uses explicitly authorized operator commands,
not a new privileged installer. Future automation needs a separately scoped issue.

## Native experiment: #131

The [diagnostic repeat](disposable-build-repeat.md) remains a failed artifact
acceptance: Cargo exited zero, but the file had two links and was rejected before
hashing. No accepted native checksum, export or execution is retroactively claimed.
The later [independent handoff](artifact-handoff.md) and
[disposable tests](../scripts/test_rehearsal_artifact_handoff.py) resolve the
copying contract; they are not a repeat of that VM experiment.

The accepted [Wasm decision](decisions/0004-first-wasm-web-app.md), implemented
[build workflow](web-app-builds.md) and real verified cloud A/B artifacts now
provide the selected delivery path. Close #131 as **not planned / superseded**,
retaining its historical failure and authorization limits. Do not run another
native build solely to turn the obsolete experiment green.

## Deployment verification still open: #19

The native executable/argument/environment wording is replaced by the accepted
[WASIp2/Wasmtime contract](decisions/0004-first-wasm-web-app.md). The artifact is a
component, and a fixed, separately installed Wasmtime worker runs under systemd
`DynamicUser`. The root broker has a narrow operator-installed interface; the
guest has no inherited environment or general command interface. This is not a
second native deployment backend.

Existing [broker tests](../scripts/test_deployment_broker.py) cover immutable
promotion, changed bytes/provenance, stale fingerprints/generations, failed
health, running/stopped intent and stop failure. They mock systemd launch/state/
health. The [local deployment rehearsal](web-app-deployments.md#disposable-local-evidence)
and cloud report exercise actual startup, health, logs, stop, failure, recovery
and reboot. Neither establishes every original negative integration case.

Keep these remaining acceptance items in #19:

- On a disposable supported Linux installation, occupy the private app port with
  an unrelated fixture listener. Verify deployment cannot report Healthy, records
  failure and stops only its own worker; the unrelated listener remains alive.
- Exercise a mismatched/missing invocation marker and changed systemd invocation.
  Verify status cannot accept another process's response as the approved worker.
  Exercise unconfirmed stop/nonempty-cgroup handling so it cannot be reported as
  Stopped or treated as permission for another deployment.
- Record the reserved fixed-unit ownership assumption and observed results. The
  [broker](../scripts/deployment_broker.py) targets fixed unit/cgroup names and
  checks `InvocationID`; it does not signal a persisted PID. Literal PID-reuse
  tests for a nonexistent PID-based supervisor are superseded, not proof of
  arbitrary foreign-unit safety.

Do not relax limits, kill a port owner's PID, add a native backend or claim these
cases passed from mocks. Selecting or running new execution fixtures requires
their own scoped validation authority; this reconciliation performs none.

## Release boundary

[#21](https://github.com/devoption/devoption/issues/21) remains open. The connected
functional path is proven, while #19's remaining evidence, the focused release
security review, [repository settings decisions](maintainers.md#settings-that-still-need-owner-decisions),
version/notes preparation and exact-commit release checks remain. The release
process still requires separate owner approval before tagging/publication.
