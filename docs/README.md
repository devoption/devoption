# Documentation

DevOption is a deployable software factory intended for individuals and teams.
The current implementation is an shared-workspace development preview with loopback HTTP
access and an explicit [HTTPS proxy mode](public-linux-install.md) with
[fresh cloud acceptance evidence](public-cloud-acceptance.md). An opt-in [Linux build workflow](web-app-builds.md) compiles the selected trusted
Wasm app through the interface. An opt-in app runtime supports saved releases and same-node deployment. The owner can invite [workspace members](users.md) or approve registration
applications; project-specific roles remain planned.

## Use and contribute

- [Shared context entry point](../AGENTS.md), [product](product.md),
  [architecture](architecture.md), [decisions](decisions/README.md) and
  [task/handoff template](task-template.md); [priorities](../ROADMAP.md)
- [First web app: install, build, deploy and maintain](first-web-app.md)
- [Unified fresh-host app-capable source installer and preservation rules](app-installation.md)
- [Public cloud acceptance evidence](public-cloud-acceptance.md) and [issue reconciliation](single-node-acceptance-status.md)
- [Deployment collision, invocation and cleanup evidence](deployment-identity-validation.md)
- [Clean local installation, app lifecycle and recovery evidence](clean-local-rehearsal.md)
- [Quick start](../README.md#quick-start)
- [Current preview user guide](using-devoption.md)
- [Authenticated read-only MCP and client registration](mcp.md)
- [Same-node snapshot and staged recovery](same-node-recovery.md)
- [Private Ubuntu installation, backup and recovery](private-linux-install.md)
- [Public-origin HTTPS installation and validation limits](public-linux-install.md)
- [Verified Ubuntu bundle installer, safe retries and validation limits](bundle-installation.md)
- [Project environment secrets and key custody](project-secrets.md)
- [Independent project app deployments and hostnames](multiple-apps.md)
- [Deployment-bound secrets and scoped HTTPS integrations](app-https.md)
- [Same-schema preview upgrades and full-state recovery](platform-upgrades.md)
- [Signed preview upgrade, interruption and reboot acceptance](bundle-upgrade-acceptance.md)
- [Ubuntu HTTPS system-service and reboot rehearsal](public-linux-rehearsal.md)
- [Development setup and checks](development.md)
- [Screenshot gallery](screenshots/gallery.md) and [capture guide](screenshots/README.md)
- [Overview design concept](design/352-overview/README.md) — proposed image, not implemented UI
- [Contribution guide](../CONTRIBUTING.md) and [maintainer checklist](maintainers.md)
- [Release process and approval gates](releases.md)
- [Local platform bundle candidates and remaining trust gates](release-bundles.md)
- [Opt-in authenticated bundle provenance and verification](bundle-provenance.md)
- [Reviewed candidate reservations and immutable retry rules](release-candidates.md)
- [Candidate retention planning, approved writer and retry recovery](release-reconciliation.md)
- [Candidate readiness, canary thresholds and promotion stop](release-readiness.md)

## Implementation references

- [Experimental app-scoped append-only PostgreSQL records](app-records.md)

- [Local mutation and execution trust contract](execution-trust.md)
- [Owner setup and deferred browser enrollment](local-enrollment.md)
- [Display-name Profile page](profile.md)
- [Browser session validation](browser-session-check.md)
- [Create server-managed repositories](managed-repositories.md)
- [Review and recover changed repository device bindings](repository-recovery.md)
- [Project catalog and HTTP operations](project-catalog.md)
- [Browse tracked repository files](repository-files.md)
- [Worktree validation](worktree-validation.md)
- [Project Builds source preparation](build-preparation.md)
- [Typed review context](review-context.md), [read-only directory verification](reviewed-directories.md)
  and [reviewed executable verification](reviewed-executables.md)
- [Real leaf-process supervision fixtures](process-supervision-fixtures.md) and
  [Linux orphan cleanup evidence](descendant-supervision-fixture.md)
- [Build plans](build-plans.md), [approval lifecycle](build-approvals.md), [job lifecycle](build-jobs.md)
  with [private history](build-history.md), [bounded output](build-output.md) and [supervision decisions](build-supervision.md)
- [Registry contract](registry-contract.md), [Cargo inputs](cargo-inputs.md) and [source proofs](cargo-source-proof.md)
- [Complete immutable source snapshots](source-snapshots.md)
- [Artifact handoff](artifact-handoff.md)
- [Build the selected Wasm web app](web-app-builds.md)
- [Deploy a web app on the same node](web-app-deployments.md)
- [App hostnames and local HTTPS status](app-hostnames.md)

## Validation evidence

These guides record bounded fixture checks and their limits; connected app lifecycle
evidence is linked under [Use and contribute](#use-and-contribute).

- [Offline Cargo fixture build](offline-fixture-build.md)
- [Linux installation rehearsal](linux-rehearsal.md) and [unattended rehearsal](linux-unattended-rehearsal.md)
- [Disposable runner proposal](disposable-runner-proposal.md), [initial build](disposable-build-rehearsal.md) and [diagnostic repeat](disposable-build-repeat.md)
