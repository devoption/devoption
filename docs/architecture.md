# Architecture

This page owns the implementation map and data flow. Read the
[product context](product.md) for purpose, [decision 0001](decisions/0001-platform-boundaries.md)
for architectural direction, and the [roadmap](../ROADMAP.md) for priorities.
Verify current code when starting work; planned capabilities are not runtime behavior.

## Implemented structure

The Rust workspace has six crates. The running service uses
`devoption-cli` → `devoption-server`, which composes `devoption-projects`,
`devoption-builds` and `devoption-ui`. The project domain core is independent of those delivery layers:

| Crate | Responsibility |
| --- | --- |
| `devoption-builds` | Source preparation observations, immutable Unix build plans, in-memory approval/job lifecycle records and bounded Cargo source membership proofs; no production approval adapter or execution |
| `devoption-cli` | Arguments, command dispatch, process diagnostics, and service startup |
| `devoption-projects` | Project/repository identities, associations, private catalog persistence, and bounded worktree inspection and explicitly managed creation |
| `devoption-registry` | Cargo input validation, dependency metadata, checked provider contract and verified single-crate seed bytes; no acquisition service |
| `devoption-server` | HTTP/runtime behavior, request guards, authentication, sessions, and owner storage |
| `devoption-ui` | HTML rendering and browser assets |

Server APIs live under `devoption_server`; the CLI is a binary without a library
facade. Authentication and owner storage remain server modules. Executable
integration tests stay with the CLI; domain logic remains in domain crates.

Use focused references for details rather than loading every subsystem:

- [Catalog persistence](project-catalog.md), [worktree inspection](worktree-validation.md)
  and [managed creation](managed-repositories.md) own repository access/storage rules.
- [Source preparation](build-preparation.md) reports observations without creating
  a plan or job. The explicit CLI check does not read owner credentials or start HTTP.
- [Plans](build-plans.md) and [job lifecycle](build-jobs.md) are contracts, not an
  execution service. [Complete snapshots](source-snapshots.md) retain verified
  source bytes; the registered collector has no HTTP/CLI integration.
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
   Snapshot, plan, job, registry and artifact contracts have separate fixture
   evidence; they are not a connected application build/deploy service. See the
   focused references above for each boundary.

## Planned flow and unresolved boundaries

The [execution trust contract](execution-trust.md) owns the required approval,
environment, storage, resource and output policy. Its future requirements are not
implemented runner guarantees.

The [roadmap](../ROADMAP.md) owns the intended
reviewed-source → approved build → verified artifact → approved deployment →
observable application sequence. Connecting those stages requires explicit
identity, approval, persistence and execution boundaries; today's observation
and contract code does not implement them.

[Product goals](product.md) define optional MCP parity with human operations.
MCP has no implemented transport here and must not become a permission bypass.
Wasm runtime/ABI and other unsettled choices remain
[open questions](decisions/README.md#open-questions), not implied dependencies.
