# DevOption platform roadmap

DevOption is being built as a self-hosted software factory for individuals and
teams: a connected path from source to a running application. This roadmap covers
platform capabilities, not company milestones or projects built with DevOption.

The sequence below expresses direction, not delivery dates or release promises.
GitHub issues define individual changes; existing milestone definitions and release
gates remain unchanged.

## Available today

The development preview supports:

- One owner account, saved projects, project creation and renaming, and browser
  appearance preferences.
- Empty repository creation in explicitly configured storage, attachment of
  existing checkouts, tracked-file browsing and Git status inspection. Separate
  projects can each have a repository with the same name.
- Read-only source preparation checks in Builds and the CLI, plus documented
  private Ubuntu source installation, backup and recovery procedures.

The preview binds to loopback. There is no published release, supported public
hosting configuration, application build/deployment service, Wasm runtime, MCP
interface or team access yet. See the [current user guide](docs/using-devoption.md)
for supported operations.

Existing [build contracts](docs/build-plans.md), immutable source verification and
fixture checks are groundwork for native Rust binaries. They do not provide a
usable application build service or implemented Rust-to-Wasm builds. Those
contracts must evolve to describe Wasm targets, artifacts and runtime requirements.

## Next: a usable single-server MVP

The first complete workflow should let one owner take one trusted Rust application
from a reviewed source revision to a verified artifact and an explicitly approved
deployment on one Linux server.

- **Rust-to-Wasm builds.** Pin source, dependencies and toolchain; define the
  supported WebAssembly (Wasm) target and host interfaces; evolve the native build
  contracts accordingly. Add approved jobs with bounded resources, useful logs,
  cancellation and recoverable status. Build-time access needs its own controls;
  Wasm runtime restrictions do not isolate the compiler or dependency build scripts.
- **Verified artifacts.** Preserve immutable outputs with checksums and provenance
  tying them to reviewed source and build inputs. Verify the exact artifact before
  promotion or execution, and retain previous versions for rollback.
- **A capability-scoped Wasm runtime.** Run applications with explicit grants for
  filesystem, network, configuration and other host services, plus resource limits
  and reliable stop/restart behavior. Define and test the supported execution
  boundary before making isolation claims.
- **Application deployment and operations.** Select a verified artifact and its
  permissions, approve deployment, and observe health and logs. Add routing, domain
  configuration and TLS as explicit deployment capabilities, with controlled
  updates, failure handling and rollback to a previous verified version.
- **Installation, upgrades and recovery.** Make single-server setup repeatable,
  preserve application and platform data through upgrades, and test backups,
  compatible rollback and recovery after interruption. Publish supported operating
  limits and recovery instructions alongside release checks.

A usable path includes human review and security gates before builds, artifact
promotion and deployment. The initial scope is trusted code and one server;
public exposure and broader workload support need their own verified boundaries.

## Optional MCP, across the platform

A built-in, optional Model Context Protocol (MCP) interface is planned as a
first-class way to use the same platform capabilities alongside the UI and CLI.
It should grow with those capabilities, starting with read-only project, source
and status access. Later, add scoped build and deployment operations only when
those workflows are available and their approval rules are enforced.

MCP operations must use the same identity, access scope, review, approval and audit
rules as UI/CLI operations. Connecting a client must not grant extra permissions or
turn a read into execution. Access should be explicitly enabled and revocable.
Human-only workflows must remain complete without an AI account or an MCP client.
Compatibility will be documented and tested per integration; MCP support does not
promise compatibility with every AI client or subscription.

## Later direction

Once the single-server path is usable, extend the platform around it:

- **Integrated source, review and pipelines.** Connect source editing and Git
  workflows with changesets, reviews, repeatable pipelines and visible results.
  Carry source identity and approvals through testing, dependency/vulnerability
  checks, artifact promotion and deployment, with enforceable security gates.
- **Registries and packages.** Grow the existing dependency contract into verified
  acquisition and storage, private and public package registries, and controlled
  upstream proxy/cache services. Preserve package origin and integrity, apply
  publishing and access policies, and keep dependency credentials out of builds.
  Registry hosting, proxy fetching and package publication are intentions, not
  current services.
- **Teams and access.** Add multiple users, project roles, scoped service access,
  invitations, revocation and shared audit history. Apply those permissions
  consistently to source, pipelines, packages, deployments and MCP.
- **Multi-server orchestration.** Build on proven single-server operations to add
  server enrollment, workload placement, coordinated rollouts, health monitoring
  and recovery across servers, while retaining explicit access and resource bounds.

Progress is tracked through [repository issues](https://github.com/devoption/devoption/issues)
and the [changelog](CHANGELOG.md). Implemented behavior belongs in the user guide;
planned capabilities stay here until they are available and validated.
