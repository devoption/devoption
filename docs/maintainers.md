# Repository maintenance

## Main branch protection

With owner authorization under [#2](https://github.com/devoption/devoption/issues/2),
GitHub branch protection was enabled and read back on 2026-10-10:

- Changes to `main` require a pull request, including for administrators.
- `Rust and repository checks` and `Issue and change documentation` must pass;
  both must come from GitHub Actions. The PR branch must be up to date.
- Zero approving reviews are required, so a sole maintainer can merge a PR
  after CI passes. No bypass actors were added.
- Force pushes and branch deletion are disabled.

These settings live in GitHub, not in the workflow files. Review changes to
workflows and validation scripts carefully; passing checks alone are not review.

## Settings that still need owner decisions

Private vulnerability reporting, secret scanning, and push protection remain
disabled. No unrelated security settings or credentials were changed. An owner
can separately review and authorize the following:

- Enable private vulnerability reporting and update `SECURITY.md` once verified.
- Review secret scanning/push protection availability and Actions settings.
  Keep default token permissions read-only and require approval for external
  contributors' workflow runs as appropriate.

These remaining settings are recommendations, not enabled protections.

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
