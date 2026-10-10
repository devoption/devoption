# Repository maintenance

## Settings that still need owner decisions

At bootstrap, no repository rulesets or private vulnerability reporting were
enabled. Secret scanning and push protection were also reported disabled.
This work does not change repository security settings or create credentials.

Before accepting outside contributions, an owner should separately review and
authorize the desired settings:

- Protect `main`: require PRs, at least one approval, resolved review discussions,
  and the `Rust and repository checks` and `Issue and change documentation`
  checks. Require a current branch and prevent force pushes and deletion.
  Consider review requirements for workflow/security changes and restrict bypass.
- Enable private vulnerability reporting and update `SECURITY.md` once verified.
- Review secret scanning/push protection availability and Actions settings.
  Keep default token permissions read-only and require approval for external
  contributors' workflow runs as appropriate.

Until enabled, these are desired protections, **not enforced guarantees**.
Changes to workflows or validation scripts need careful human review too.

## Verified upstream pins

Verified against official sources on 2026-10-10:

- [Rust 1.99.0](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/), pinned in
  `rust-toolchain.toml`. Local tests use the matching installed compiler.
- [actions/checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1)
  resolves to `3d3c42e5aac5ba805825da76410c181273ba90b1` in the official
  `actions/checkout` repository. Every use disables credential persistence.

Actions use full commit SHAs following
[GitHub's security guidance](https://docs.github.com/en/actions/reference/security/secure-use).
Ubuntu runner images and their included tools still change over time; the
workflow does not claim a fully hermetic build. The Rust crate has no third-party
dependencies, and Python automation uses only the standard library.

Create an issue before updating pins, dependencies, or tooling. Validate workflows
with [actionlint](https://github.com/rhysd/actionlint), verify upstream release
checksums for downloaded tools, run the documented checks, and inspect GitHub CI.
Release validation and publication steps are in [releases.md](releases.md).
