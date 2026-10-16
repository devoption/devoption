# DevOption platform roadmap

DevOption is being built as a self-hosted software factory for individuals and
teams: a connected path from source to a running application. This roadmap covers
platform capabilities, not company milestones or projects built with DevOption.

DevOption is for developers and small teams who want a self-hosted, auditable path
from reviewed source to a running service on their own servers, without relying on
hosted CI/CD or container orchestration. The focus is explicit review and approval,
immutable provenance for artifacts, and a capability-scoped runtime. It aims to connect the
steps you already trust — source, checks, build, and deploy — without adding new
promises beyond what is implemented and validated here.

The sequence below expresses direction, not delivery dates or release promises.
GitHub issues define individual changes. The immediate functional delivery track
below takes priority; product release approval remains a separate gate.

## Available today

The development preview supports:

- One owner account, saved projects, project creation and renaming, and browser
  appearance preferences.
- Empty repository creation in explicitly configured storage, attachment of
  existing checkouts, tracked-file browsing and Git status inspection. Separate
  projects can each have a repository with the same name.
- Read-only source preparation checks in Builds and the CLI, plus documented
  private Ubuntu source installation, backup and recovery procedures.
- Authenticated project build-history and log views, with selected history recovery
  at server startup; these views do not run builds.

The preview binds to loopback and has an explicit HTTPS proxy mode with a
[public installation guide](docs/public-linux-install.md).
[Local Ubuntu HTTPS and reboot checks](docs/public-linux-rehearsal.md) passed;
real cloud DNS/ACME and fresh-host validation remain pending. There is no published
release, application
build/deployment service, Wasm runtime, MCP interface or team access yet. See the [current user guide](docs/using-devoption.md)
for supported operations.

Existing [build contracts](docs/build-plans.md), immutable source verification and
fixture checks are groundwork for native Rust binaries. They do not provide a
usable application build service or implemented Rust-to-Wasm builds. Those
contracts must evolve to describe Wasm targets, artifacts and runtime requirements.

## Immediate priority: one VM, platform DNS, deployed web-app DNS

The next functional milestone is the owner's complete single-node path, tracked
in [#228](https://github.com/devoption/devoption/issues/228):

1. Create a cloud VM and install DevOption.
2. Point a platform DNS name at the server and log into DevOption over HTTPS.
3. Create a project for a web app and supply its source.
4. Build and deploy that app to the same node through the platform interface.
5. Point a second DNS name at the same server and route it to the app.
6. Access the app through that name, deploy an update, and keep it usable after restart.

This path now takes priority over broader platform expansion and optional
maintenance. Each implementation task must advance one of these steps or remove
an evidenced blocker to it. Prefer connecting existing components over adding
more standalone contracts. Keep individual PRs reviewable, but complete each
user-visible capability before moving to optional improvements.

### Delivery sequence

| Task | Observable completion | Dependencies |
| --- | --- | --- |
| [#229: Minimum web-app contract](https://github.com/devoption/devoption/issues/229) | Select one Wasm runtime, target/ABI and HTTP interface, plus a tiny Rust web app and exact source/dependency preparation path. Reconcile existing native build assumptions. | First implementation-enabling decision |
| [#230: Install and log in through DNS](https://github.com/devoption/devoption/issues/230) | Fresh Ubuntu VM, persistent platform service, configured public origin, HTTPS ingress, owner setup and working authenticated project creation. | Can proceed independently of the runtime choice |
| [#231: Build from the project interface](https://github.com/devoption/devoption/issues/231) | Review and approve a real build of the selected app; see durable outcomes/logs and obtain the verified Wasm output. | #229; #230 for public UI validation |
| [#232: Release and deploy on the node](https://github.com/devoption/devoption/issues/232) | Promote a verified artifact, deploy it, inspect health/logs, stop it, update it and recover the prior version; reconcile after restart. | #229 and #231 |
| [#233: App DNS and HTTPS](https://github.com/devoption/devoption/issues/233) | A second hostname reaches the app on the same VM, stays stable through updates/restart, and remains separate from platform control. | #230 and #232 |
| [#234: Fresh-VM acceptance](https://github.com/devoption/devoption/issues/234) | One repeatable runbook and real evidence for all six steps, a second revision, failure recovery, reboot and backup/restore. | #230–#233 |

The series integrates the existing build [#17](https://github.com/devoption/devoption/issues/17),
artifact [#18](https://github.com/devoption/devoption/issues/18), deployment
[#19](https://github.com/devoption/devoption/issues/19) and installation/recovery
[#20](https://github.com/devoption/devoption/issues/20) work. It does not restart
those implementations. For this track, #229 resolves their native-only assumptions
against the accepted [Wasm-first direction](docs/decisions/0001-platform-boundaries.md),
and #230/#233 explicitly add public hosting beyond their earlier local-only scope.
Unaffected integrity, authorization and recovery criteria remain applicable.

### Keep the first path small

Start with one owner, one Ubuntu 24.04 x86_64 VM and one trusted Rust HTTP web app.
The first task selects the [Wasm contract](docs/decisions/0004-first-wasm-web-app.md);
see its review status. It is not an already supported runtime.
A minimal app serving HTML and a health endpoint is enough to establish the path;
arbitrary web frameworks, databases and untrusted workloads are not prerequisites.
The owner may create the VM and DNS records manually and stage the source checkout
over SSH. Reuse project creation and repository attachment rather than requiring
an editor, Git hosting, cloud provisioning API or registry service first.

Use one explicit reverse-proxy/TLS setup for the two hostnames. Keep platform and
app upstreams private; expose the documented HTTPS entry points. Automated domain
management may follow a working operator-managed setup. Public-origin request
validation, authenticated control, HTTPS, bounded builds/runtime and artifact
integrity belong in this first path. Broader hardening and scale work can follow;
a loopback-only demonstration does not satisfy the requested outcome.

Completion means the platform and app are reachable by their separate DNS names,
the owner can deploy a changed revision at the same app URL, and reboot/recovery
behavior has been demonstrated. Source installation is sufficient: this does not
wait for product package publication, and it does not claim general production
readiness. The provisional [v0.1.0 milestone](https://github.com/devoption/devoption/milestone/1)
and [release approval gate #21](https://github.com/devoption/devoption/issues/21)
remain separate from functional acceptance.

Themes, UI refactoring, optional authentication maintenance, MCP, package hosting,
teams, configurable workflow engines and multi-server orchestration are deferred
unless a concrete blocker to this path is established. Creating the task series
is not permission to spend money, create credentials, change DNS or repository
security, execute real workloads, merge changes or publish releases.

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

## How to help

- Start with open issues labeled
  [good first issue](https://github.com/devoption/devoption/issues?q=is%3Aopen+label%3A%22good%20first%20issue%22)
  or
  [help wanted](https://github.com/devoption/devoption/issues?q=is%3Aopen+label%3A%22help%20wanted%22).
- Support development via [GitHub Sponsors](https://github.com/sponsors/devoption).
