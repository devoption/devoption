# Architecture

This page owns the implementation map and data flow. Read the
[product context](product.md) for purpose, [decision 0001](decisions/0001-platform-boundaries.md)
for architectural direction, and the [roadmap](../ROADMAP.md) for priorities.
Verify current code when starting work; planned capabilities are not runtime behavior.

## Implemented structure

The Rust workspace has seven crates. The running service uses
`devoption-cli` → `devoption-server`, which composes `devoption-projects`,
`devoption-builds` and `devoption-ui`. The project domain core is independent of those delivery layers:

| Crate | Responsibility |
| --- | --- |
| `devoption-builds` | Source preparation observations, immutable Unix build plans, in-memory approval/job lifecycle records and bounded Cargo source membership proofs; opt-in reviewed Linux Wasm build execution (see [build workflow](web-app-builds.md)) |
| `devoption-cli` | Arguments, command dispatch, process diagnostics, and service startup |
| `devoption-projects` | Project/repository identities, associations, private catalog persistence, and bounded worktree inspection and explicitly managed creation |
| `devoption-registry` | Cargo input validation, dependency metadata, checked provider contract and verified single-crate seed bytes; no acquisition service |
| `devoption-runtime` | Separate Wasmtime WASIp2 HTTP worker; private listener, fresh bounded Store per request, denied filesystem/environment/socket/DNS grants and opt-in [host-mediated HTTPS](app-https.md) |
| `devoption-server` | HTTP/runtime behavior, request guards, authentication, independent account sessions, owner storage and durable member approvals/invitations |
| `devoption-ui` | HTML rendering and browser assets |

Server APIs live under `devoption_server`; the CLI is a binary without a library
facade. Authentication, [shared workspace users](users.md), owner storage and the opt-in [project environment secret store](project-secrets.md) remain server modules. Executable
integration tests stay with the CLI; domain logic remains in domain crates.

Use focused references for details rather than loading every subsystem:

- [Catalog persistence](project-catalog.md), [worktree inspection](worktree-validation.md)
  and [managed creation](managed-repositories.md) own repository access/storage rules.
- [Source preparation](build-preparation.md) reports observations without creating
  a plan or job. The explicit CLI check does not read owner credentials or start HTTP.
- [Plans](build-plans.md) and [job lifecycle](build-jobs.md) are contracts, not an
  execution service. [Private history](build-history.md) persists bounded summaries
  and per-attempt output checkpoints without restoring execution authority.
  Explicit server startup can recover selected history before listening. [Bounded output](build-output.md) collects supplied bytes and
  renders escaped text without process readers. Authenticated project pages read
  saved attempts and bounded checkpoints through fresh catalog scoping checks.
  [Supervision decisions](build-supervision.md) track stop reasons and grace
  without delivering signals or asserting cleanup.
  [Complete snapshots](source-snapshots.md) retain verified source bytes; the Linux build adapter uses their validated package/library identities for
  [trusted standalone Wasm apps](web-app-builds.md#application-identity-and-assets).
- [Registry contracts](registry-contract.md), [Cargo inputs](cargo-inputs.md) and
  [source proofs](cargo-source-proof.md) validate supplied data; they do not provide
  acquisition, registry hosting or build permission.
- [Offline fixture builds](offline-fixture-build.md) and
  [artifact handoff](artifact-handoff.md) provide bounded evidence, not production
  execution or isolation guarantees. The [documentation index](README.md#validation-evidence)
  links rehearsal results and their limits.
- [Reviewed-directory verification](reviewed-directories.md) opens and rechecks
  private directory identities without provisioning, execution or approval integration.
  [Reviewed-executable verification](reviewed-executables.md) separately retains
  and hashes the two reviewed tool files without launching them.

## Implemented data flow

1. The [CLI](../crates/devoption-cli/src/main.rs) selects an explicit command and
   configuration. `serve` starts the [HTTP service](../crates/devoption-server/src/server.rs),
   which validates configured owner/catalog storage before binding to loopback.
   Optional public-origin mode requires an existing owner and an explicit local TLS
   proxy; see [public installation](public-linux-install.md) for the boundary.
   CLI owner setup is separate; serving does not enroll an owner.
2. The server applies Host checks, configured-page session checks and mutation
   Origin/CSRF checks. It invokes domain operations and supplies results to
   [UI rendering](../crates/devoption-ui/src/lib.rs); browser assets handle client
   interactions and appearance preferences. Exact protocol rules live in the
   [user guide](using-devoption.md#http-authentication-in-the-current-preview).
3. [Project handlers](../crates/devoption-server/src/server/projects.rs) use the
   project catalog and explicit repository-root capabilities for creation,
   inspection and bounded file reads. The browser cannot grant a new root or
   turn source inspection into execution. Storage rules belong in the
   [catalog](project-catalog.md) and [managed repository](managed-repositories.md) guides.
4. Source preparation through HTTP or CLI produces read-only observations.
   With [Linux builds](web-app-builds.md) explicitly enabled, the server captures
   committed source, prepares an immutable review, consumes owner approval and
   invokes the fixed broker. Its unprivileged systemd worker compiles with verified
   offline inputs; the adapter checkpoints bounded logs, outcomes and provenance.
   Startup reconciles the owned build unit before recovering unfinished history.
5. With [deployment](web-app-deployments.md) explicitly enabled, the owner promotes
   a successful build into an immutable saved release, then separately reviews
   and approves deployment. The broker verifies provenance/artifact bytes and
   starts the bounded Wasmtime worker under a separate dynamic identity. The
   interface supplies health/log/stop controls and explicit saved-release recovery.
   Previously approved healthy running intent can recover after restart; stopped
   intent remains stopped. Optional [HTTPS bindings](app-https.md) retain exact
   environment secret revisions in review and deliver only selected values through
   systemd credentials to the host transport; guests never receive raw secrets.
6. [App hostname configuration](app-hostnames.md) binds each explicitly configured project slot to
   its own Caddy HTTPS site, listener, units and runtime state on the same IP;
   see [independent app slots](multiple-apps.md). The control service verifies its
   canonical origin; the broker checks local TLS/health for that binding.
   [Same-node recovery](same-node-recovery.md) snapshots records/artifacts offline
   and stages verified copies without overwriting the installation.

The [execution trust contract](execution-trust.md) owns the policy; the build and
runtime guides specify enforced limits and remaining trust assumptions. The
[first-app runbook](first-web-app.md) links setup and validation evidence. Local
fixture/rehearsal results do not establish public DNS/ACME acceptance or general
production readiness.

The authenticated [operational Overview](using-devoption.md#operational-overview)
combines fresh catalog associations, saved build history, and read-only broker
runtime summaries. The Wasm worker publishes bounded aggregate telemetry in its
existing host runtime directory; invocation binding and freshness checks prevent
old workers from supplying current coverage. No observation grants workload or
approval authority. MCP transport and security scans remain unimplemented.

## Planned flow and unresolved boundaries

The [roadmap](../ROADMAP.md) owns remaining public-host acceptance and broader
platform expansion beyond the selected trusted single-node app profile.

[Product goals](product.md) define the external-harness boundary and optional MCP
parity with human operations.
MCP has no implemented transport here and must not become a permission bypass.
The first Wasm runtime/ABI and app subset are selected in
[decision 0004](decisions/0004-first-wasm-web-app.md). The deployment guide
describes the implemented private worker and its limits. Broader
capabilities remain [open questions](decisions/README.md#open-questions).
