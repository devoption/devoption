# Changelog

Changes are recorded here before release. No release has been published.
Entries describe implemented changes, not the roadmap.

## [Unreleased]

### Added

- A Dark/Light selector inside Appearance, defaulting to Nord dark-purple and
  saving the choice in browser-local storage, with safe invalid/blocked-storage
  fallbacks and a readable light palette. Added dependency-free script/contrast
  tests to CI and an actual light-mode screenshot; recaptured all default pages
  ([#29](https://github.com/devoption/devoption/issues/29)).
- An empty Projects page at `/projects` in the shared navigation, with active-page
  state, request tests, and updated screenshots of all three main pages
  ([#12](https://github.com/devoption/devoption/issues/12)).
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

- Recreated the approved sidebar/header shell, with spacious content, responsive
  navigation, and a compact local-preview popup containing read-only Appearance
  information; refreshed all main-page screenshots
  ([#27](https://github.com/devoption/devoption/issues/27)).
- Extracted the existing renderer and CSS/SVG assets into a dependency-free
  `devoption-ui` library; the CLI keeps HTTP handling and remains the only binary.
  Page output and screenshots are unchanged
  ([#25](https://github.com/devoption/devoption/issues/25)).
- Put the README's contribution links in a list and separate its sponsorship link
  for easier reading; document the owner-authorized PR-and-CI protection now
  enabled on `main`
  ([#2](https://github.com/devoption/devoption/issues/2)).
