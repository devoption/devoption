# Repository maintenance

## Main branch protection

With owner authorization under [#2](https://github.com/devoption/devoption/issues/2),
GitHub branch protection was enabled on 2026-10-10 and read back again on
2026-10-17 (America/New_York):

- Changes to `main` require a pull request, including for administrators.
- `Rust and repository checks` and `Issue and change documentation` must pass;
  both must come from GitHub Actions. The PR branch must be up to date.
- Zero approving reviews are required, so a sole maintainer can merge a PR
  after CI passes. No bypass actors were added.
- Force pushes and branch deletion are disabled.

These settings live in GitHub, not in the workflow files. Review changes to
workflows and validation scripts carefully; passing checks alone are not review.

## Verified repository settings

A read-only GitHub API check on 2026-10-17 (America/New_York) under
[#256](https://github.com/devoption/devoption/issues/256) observed:

| Setting | Observed state |
| --- | --- |
| Private vulnerability reporting | Enabled; use the private form in [SECURITY.md](../SECURITY.md) |
| Secret scanning | Enabled |
| Secret-scanning push protection | Enabled |
| Dependabot security updates | Enabled |
| GitHub Actions | Enabled; all actions allowed; repository SHA pinning requirement disabled |
| Repository default workflow token permission | `write` |
| Actions may approve pull requests | `true` |

The values came from `GET /repos/devoption/devoption`,
`GET /repos/devoption/devoption/private-vulnerability-reporting`,
`GET /repos/devoption/devoption/actions/permissions` and
`GET /repos/devoption/devoption/actions/permissions/workflow`. These are snapshots,
not continuous enforcement or evidence that no secrets exist. This reconciliation
changed no setting and does not establish when or by whom they were enabled.

## Settings that still need owner decisions

Ordinary PR workflows and release proposal jobs explicitly declare
`contents: read`, pin external actions to full SHAs and disable checkout credential
persistence. The optional [trusted-main draft writer](release-prs.md) is the
explicit exception: its opted-in job has limited contents/PR write access and
issue read access, with no merge or publication operation. That source
policy does not make the repository's default workflow token read-only or require
every future action to be SHA-pinned.

Under [#21](https://github.com/devoption/devoption/issues/21), review the observed
write default, permission to approve PRs and unrestricted action policy. Decide
whether to retain them or separately authorize changes. Review external-contributor
workflow approval policy too; that setting was not verified in this pass. No
settings change, contact address or response SLA is implied by this document.

## Verified upstream pins

Verified against official sources on 2026-10-10:

- [Rust 1.99.0](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/), pinned in
  `rust-toolchain.toml`. Local tests use the matching installed compiler.
- [actions/checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1)
  resolves to `3d3c42e5aac5ba805825da76410c181273ba90b1` in the official
  `actions/checkout` repository. Every use disables credential persistence.
- [Axum 0.8.9](https://docs.rs/axum/0.8.9/axum/) and
  [Tokio 1.53.2](https://docs.rs/tokio/1.53.2/tokio/), verified against their
  crates.io release metadata. Direct versions are exact; transitive versions and
  registry checksums are committed in `Cargo.lock`. Default features are disabled;
  the local server enables only HTTP/1 and the needed Tokio runtime/network support.

Actions use full commit SHAs following
[GitHub's security guidance](https://docs.github.com/en/actions/reference/security/secure-use).
Ubuntu runner images and their included tools still change over time; the
workflow does not claim a fully hermetic build. Fetch Rust dependencies with
`cargo fetch --locked` before running offline checks. Python automation uses only
the standard library.

Create an issue before updating pins, dependencies, or tooling. Validate workflows
with [actionlint](https://github.com/rhysd/actionlint), verify upstream release
checksums for downloaded tools, run the documented checks, and inspect GitHub CI.
Release validation and publication steps are in [releases.md](releases.md).
