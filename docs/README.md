# Documentation

DevOption is a deployable software factory intended for individuals and teams.
The current implementation is an owner-only development preview with loopback HTTP
access. Product direction does not imply that team accounts, application builds,
deployments or production hosting are already available.

## Use and contribute

- [Quick start](../README.md#quick-start)
- [Current preview user guide](using-devoption.md)
- [Private Ubuntu installation, backup and recovery](private-linux-install.md)
- [Development architecture and checks](development.md)
- [Screenshot gallery](screenshots/gallery.md) and [capture guide](screenshots/README.md)
- [Contribution guide](../CONTRIBUTING.md) and [maintainer checklist](maintainers.md)
- [Release process and approval gates](releases.md)

## Implementation references

- [Owner setup and deferred browser enrollment](local-enrollment.md)
- [Display-name Profile page](profile.md)
- [Browser session validation](browser-session-check.md)
- [Create server-managed repositories](managed-repositories.md)
- [Project catalog and HTTP operations](project-catalog.md)
- [Browse tracked repository files](repository-files.md)
- [Worktree validation](worktree-validation.md)
- [Project Builds source preparation](build-preparation.md)
- [Build plans](build-plans.md) and [job lifecycle](build-jobs.md)
- [Registry contract](registry-contract.md), [Cargo inputs](cargo-inputs.md) and [source proofs](cargo-source-proof.md)
- [Artifact handoff](artifact-handoff.md)

## Validation evidence

These guides describe bounded fixture checks, not production execution support.

- [Offline Cargo fixture build](offline-fixture-build.md)
- [Linux installation rehearsal](linux-rehearsal.md) and [unattended rehearsal](linux-unattended-rehearsal.md)
- [Disposable runner proposal](disposable-runner-proposal.md), [initial build](disposable-build-rehearsal.md) and [diagnostic repeat](disposable-build-repeat.md)
