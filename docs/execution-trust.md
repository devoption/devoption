# Local mutation and execution trust contract

Review gate: policy tracked under [#15](https://github.com/devoption/devoption/issues/15).
Merging this contract accepts requirements for dependent work; it does not enable
execution or authorize a workload. The HTTP controls below exist today. Production job
approval, work/artifact storage and resource enforcement do not. A separate
[in-memory approval lifecycle](build-approvals.md) models single-use review records
without an authenticated production adapter or execution capability.

## Trust and authority

The initial workflow has one owner running trusted code as an unprivileged Unix
OS account. The selected repository, dependency build scripts, compiler/linker,
resulting service, OS, browser and local filesystem must be trusted. A malicious
program running with that account's permissions can access its files and network;
loopback binding and Cargo `--locked --offline` are neither a sandbox nor a
filesystem/network boundary. Browser extensions and other processes under that
account are inside the trust boundary. Do not put credentials in job inputs.

The control listener remains loopback-only. Any future application runtime
listener must also remain loopback-only until a separately reviewed exposure
policy supersedes this contract. There is no runtime listener today. No public
exposure, multi-user login, repository secrets or general shell endpoint is
introduced here. Registration, inspection and source capture never run repository
code, hooks, dependency installation or builds. Selecting a root grants only the
explicit documented filesystem operations, never execution permission.

## Existing HTTP boundary and method policy

The [server guard](../crates/devoption-server/src/server.rs) requires one allowed
Host with the actual listener port, and validates absolute-form request authority.
It does not trust proxy headers or perform DNS resolution to admit hosts. Unsafe
methods require a matching Origin and a valid identity-bound session cookie plus
explicit synchronizer CSRF header; CORS is not an authorization control.
Pre-authentication endpoints instead require their documented bounded, single-use
challenge before password processing. See [HTTP authentication](using-devoption.md#http-authentication-in-the-current-preview)
for transport and session details and [local setup](local-enrollment.md) for owner creation.

GET and HEAD remain nonmutating; OPTIONS and TRACE have no mutation handlers.
Current domain mutations and authentication operations are POST routes. Unknown
methods do not gain authority by being unsupported: unsafe requests pass the
guards before dispatch. Every new route must explicitly declare its methods,
authentication/CSRF requirements and input bound, with rejection tests showing
that a denied request never reaches its mutation. Session authentication is not
job approval. Safe file reads must not start a job or implicitly register source.
This retains the existing safe-method classification; adding routes requires
reassessment as tracked in [#99](https://github.com/devoption/devoption/issues/99).

Password forms currently have a 4096-byte limit and five-second read deadline.
Other mutation bodies use bounded parsers with a five-second deadline and explicit
route-specific byte limits. Retain these controls; future endpoints must specify
and test limits before mounting. No unbounded JSON/form/log ingestion is allowed.

## Required review and approval contract

Before a future job can be approved, present one immutable review record containing:

- Owner identity, project/repository identity, exact full source commit and verified
  complete source snapshot digest; selected target and dependency bundle identity,
  hashes, size and verified registry provenance.
- Pinned toolchain version and verified executable identity, absolute executable,
  exact argument vector (no shell), and absolute owned working directory.
- Every effective environment variable and value, dependency acquisition/network
  requirements, execution identity, filesystem access and enforced resource caps.
- Output/artifact bounds, retention and cancellation behavior, and the trusted-code
  limitation above. Approval must not conceal defaults affecting execution.

The current [BuildPlan](build-plans.md) supplies part of this metadata, not an
approval record or an execution capability. The future approval issuer must bind
an explicit authenticated owner action to the complete verified record and a
single job ID. A session cookie alone, plan equality, repository registration or
a previously successful build is insufficient. Reject dirty/uncommitted source;
never build directly in the user's checkout.

An approval expires five minutes after issuance if not consumed. It is single-use:
consume it atomically when admitting that job; failure, cancellation or retry needs
fresh approval. Recheck source/catalog binding, executable/toolchain, dependencies,
paths, effective environment and caps before admission. Any changed value or failed
verification invalidates approval and requires a new review. A service restart
invalidates unconsumed approvals; never resume a job automatically. Persisted job
records cannot restore execution authority. Already admitted work remains bound to
its reviewed record; session expiry does not by itself establish process termination.
Cancellation must stop the complete process group/cgroup and confirm child cleanup.

Dependency acquisition is a separate reviewed operation through the registry/proxy.
No implicit public fetch, toolchain installation or fallback is allowed during a
build. The target/runtime choice remains subject to the [architecture decision](decisions/0001-platform-boundaries.md);
the existing native fixture command does not settle the Wasm ABI or compiler flags.
Changing target or arguments requires review and a new approval.

## Owned filesystem policy for dependent implementation

Existing owner and catalog data rules remain authoritative in
[local setup](local-enrollment.md) and [catalog storage](project-catalog.md).
Future jobs require separately configured absolute work and artifact roots owned
by the supervisor's unprivileged account, outside registered checkouts. Require
private local Unix directories (mode `0700`), no group/other access, no symlink
components, and no untrusted ACLs or inherited writer handles. Do not create,
repair or adopt an arbitrary user-selected directory through HTTP.

Validate canonical path identity and ownership at configuration, then retain
open directory descriptors. Resolve job children relative to those descriptors
with no-follow operations; reject absolute child names, dot/parent traversal,
symlinks, cross-filesystem traversal and root replacement. Canonicalization alone
is not protection from replacement races. Use server-generated job names and
exclusive creation; never recursively delete user paths or follow links on cleanup.

Each job receives a fresh owned tree with separate source, HOME, Cargo home,
scratch and target directories. Stage verified source read-only; never share a
writable Cargo cache. New private data/log files use `0600` and a restrictive umask;
accepted artifact bytes use `0400`. No artifact is executed during collection.
After complete runner termination, copy only the reviewed output into independent
private staging, validate its regular-file identity, bounds and provenance, hash
it and make it immutable before exposure. Reject symlinks and unaccounted hard-link
aliases. [Artifact handoff](artifact-handoff.md) defines the existing fixture's
stronger descriptor and alias checks; it is not a production collector.

On success or failure, remove only owned transient job trees after verifying
termination; quarantine uncertain leftovers and fail closed. Keep private bounded
logs and provenance for at most seven days, and at most ten completed jobs per
instance (oldest first). Retain at most one 4 MiB candidate artifact per retained
successful job. Promotion transfers retention to the separately reviewed release
policy in [#18](https://github.com/devoption/devoption/issues/18); never silently
remove promoted releases. If safe cleanup or storage bounds cannot be maintained,
refuse further jobs. These are future limits, not a current garbage collector.

## Environment, resources and output

Start with an empty environment. The initial build allowlist is only:

| Name | Permitted source/value |
| --- | --- |
| `PATH` | Exact reviewed trusted toolchain/system directories; no empty, relative or checkout entries |
| `RUSTC` | Absolute verified compiler path from the pinned toolchain |
| `HOME`, `CARGO_HOME`, `CARGO_TARGET_DIR`, `TMPDIR` | Exact private job directories described above |
| `LANG` | `C.UTF-8` |
| `CARGO_NET_OFFLINE` | `true` |
| `CARGO_INCREMENTAL` | `0` |
| `CARGO_BUILD_JOBS` | `1` |
| `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER` | Empty values only |

No inherited service environment, tokens, SSH agents, proxy settings, compiler
flags or caller-selected additional names. Controlled Cargo configuration and
read-only dependency materialization must also be verified; environment filtering
alone cannot neutralize repository/toolchain configuration. Reject unsupported
configuration rather than silently enabling wrappers or fetching dependencies.
BuildPlan's EnvironmentEntry validator checks syntax only. The separate
[review-context policy gate](review-context.md#initialbuildv1-proposed-settings-gate)
requires this complete explicit allowlist and checks proposed time/log caps before
approval issuance. It does not apply the environment or enforce OS limits.
Expanding the allowlist requires a reviewed policy change and new approval.

The first production build policy has one active job, at most one CPU quota,
512 MiB memory, no swap, 64 processes, 256 MiB job filesystem including staging,
60 seconds elapsed, 1 MiB combined stdout/stderr and a 4 MiB selected artifact.
Reserve staging space within that filesystem cap. These deliberately conservative
caps follow the disposable fixture profile; they are not proof a production target
will fit. Raise them only through a reviewed policy change. No claim of enforcement
is made until the selected runner tests each control, including startup and handoff.
Fail closed if a platform cannot enforce the selected policy.

Build networking must be denied by an independently tested OS boundary; Cargo's
offline flag is insufficient. The runner must hide unrelated credentials and
user storage and enforce the reviewed read/write roots. The exact isolation
mechanism remains dependent implementation work; these requirements do not make
arbitrary untrusted code supported. Do not grant the build privileged setup rights.

The 60-second execution deadline includes artifact handoff. The five-second
termination grace is additional, giving a 65-second maximum before forced kill;
cleanup verification is required before admitting another job. On cancellation,
timeout or log-cap breach,
stop the whole job, allow at most five seconds for termination, then kill remaining
children and verify no writers remain. Keep the deadline/caps through handoff;
do not reset them after Cargo exits. Mark truncated output explicitly and never
accept partial/truncated output as successful evidence. Render logs as escaped
text (including HTML and terminal control/ANSI handling), never HTML or executable
terminal sequences. Do not emit passwords, session/CSRF tokens or private paths in
HTTP errors. Trusted programs may print secrets: arbitrary secret redaction is
not promised. Private log permissions and bounded retention limit exposure but
cannot make secret-bearing output safe to publish.

## Evidence, residual risk and closure gate

Existing rejection coverage includes:

- [CLI HTTP tests](../crates/devoption-cli/tests/server.rs): deceptive/duplicate
  Host, conflicting absolute authority, cross-origin and missing-Origin requests,
  extension methods, nonmutating navigation and absent CORS authorization.

- [HTTP session tests](../crates/devoption-server/src/server/session_tests.rs):
  invalid credentials, wrong Host/Origin, expiry, rotation, logout and server identity.
- [Browser boundary tests](../crates/devoption-server/src/server/session_tests/browser.rs):
  explicit CSRF, mixed/alternate credential transport and stale credentials.
- [Pre-authentication tests](../crates/devoption-server/src/server/session_tests/preauth.rs)
  and [password-form tests](../crates/devoption-server/src/password_form/tests.rs):
  POST/Host/Origin rejection before work, single-use challenges and bounded input.
- [CLI integration tests](../crates/devoption-cli/tests/setup.rs),
  [owner storage tests](../crates/devoption-server/src/owner_store/tests.rs) and
  [project tests](../crates/devoption-projects/src/tests/storage.rs): terminal setup,
  private storage, create-once and unsafe-path rejection.

Run the [development checks](development.md#checks); record the exact reviewed
commit and CI results in the PR. These tests demonstrate current controls, not
future approval invalidation, isolation or process cleanup. Dependent issues must
add their own real success/failure, stale-approval, resource, cancellation, restart,
output escaping and filesystem-preservation tests before enabling execution.

Residual risks include same-account compromise, malicious trusted code/browser
extensions, local denial of service against shared authentication capacity,
filesystem races outside the stated trusted-writer model, and secret-bearing
program output. The optional authentication decisions in #99 are not all closure
requirements for #15. The method policy above must continue to be checked for each
new route. Loopback access is not proof of a caller's OS identity.

Close #15 only after the policy PR is reviewed and merged with passing required
checks. Leave execution disabled: [#17](https://github.com/devoption/devoption/issues/17)
owns build approval/runner integration, [#18](https://github.com/devoption/devoption/issues/18)
artifact promotion and [#19](https://github.com/devoption/devoption/issues/19)
runtime approval/supervision. The separately approved rehearsal in
[#131](https://github.com/devoption/devoption/issues/131) is evidence work, not
production authority. No executor, shell endpoint, deployment or workload is
introduced or authorized by this document.
