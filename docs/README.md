# Documentation

DevOption is a deployable software factory intended for individuals and teams.
The current implementation is an owner-only development preview with loopback HTTP
access and an explicit [HTTPS proxy mode](public-linux-install.md) awaiting cloud validation. An opt-in [Linux build workflow](web-app-builds.md) compiles the selected trusted
Wasm app through the interface. Team accounts, app deployment and production
hosting remain planned.

## Use and contribute

- [Shared context entry point](../AGENTS.md), [product](product.md),
  [architecture](architecture.md), [decisions](decisions/README.md) and
  [task/handoff template](task-template.md); [priorities](../ROADMAP.md)
- [Quick start](../README.md#quick-start)
- [Current preview user guide](using-devoption.md)
- [Private Ubuntu installation, backup and recovery](private-linux-install.md)
- [Public-origin HTTPS installation and validation limits](public-linux-install.md)
- [Ubuntu HTTPS system-service and reboot rehearsal](public-linux-rehearsal.md)
- [Development setup and checks](development.md)
- [Screenshot gallery](screenshots/gallery.md) and [capture guide](screenshots/README.md)
- [Contribution guide](../CONTRIBUTING.md) and [maintainer checklist](maintainers.md)
- [Release process and approval gates](releases.md)

## Implementation references

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

## Validation evidence

These guides describe bounded fixture checks, not production execution support.

- [Offline Cargo fixture build](offline-fixture-build.md)
- [Linux installation rehearsal](linux-rehearsal.md) and [unattended rehearsal](linux-unattended-rehearsal.md)
- [Disposable runner proposal](disposable-runner-proposal.md), [initial build](disposable-build-rehearsal.md) and [diagnostic repeat](disposable-build-repeat.md)

- [Deploy a web app on the same node](web-app-deployments.md): private Wasm releases, runtime setup, review, health, stop and recovery.

- [App hostnames](app-hostnames.md): generate the two-host Caddy configuration and inspect local HTTPS routing/certificate status.
