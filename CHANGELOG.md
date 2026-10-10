# Changelog

Changes are recorded here before release. No release has been published.
Entries describe implemented changes, not the roadmap.

## [Unreleased]

### Added

- Tracked Home and About screenshots in the README, with fixed-viewport capture
  instructions and a requirement to refresh affected images in UI-changing commits
  ([#10](https://github.com/devoption/devoption/issues/10)).
- The approved DevOption SVG in the shared header, with original path geometries,
  primary-color `dev` lettering, and basic Nord-purple color/spacing/type values
  ([#8](https://github.com/devoption/devoption/issues/8)).
- Shared HTML layout and working Home/About navigation with active-page state,
  a keyboard skip link, and explicit empty content; request tests cover both pages
  ([#6](https://github.com/devoption/devoption/issues/6)).
- A localhost-only `devoption serve` command with plain-text GET/HEAD responses,
  HTTP error handling, configurable port, startup errors, and real request tests;
  locked Axum/Tokio dependencies and a CI fetch step
  ([#4](https://github.com/devoption/devoption/issues/4)).
- A dependency-free Rust workspace and CLI scaffold with help/version output,
  explicit unsupported-input errors, and command-level tests
  ([#1](https://github.com/devoption/devoption/issues/1)).
- MIT licensing for DevOption Labs LLC, tested setup instructions, contribution
  and security guidance, issue/PR templates, and GitHub sponsorship metadata
  ([#1](https://github.com/devoption/devoption/issues/1)).
- Read-only GitHub CI for formatting, linting, tests, issue-linked contribution
  metadata, and changelog changes; manual release validation that extracts dated,
  version-matched notes without publishing
  ([#1](https://github.com/devoption/devoption/issues/1)).

### Changed

- Put the README's contribution links in a list and separate its sponsorship link
  for easier reading; document the owner-authorized PR-and-CI protection now
  enabled on `main`
  ([#2](https://github.com/devoption/devoption/issues/2)).
