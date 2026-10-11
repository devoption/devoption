# Changelog

Changes are recorded here before release. No release has been published.
Entries describe implemented changes, not the roadmap.

## [Unreleased]

### Added

- Optional Unix `serve --data-dir` configuration now validates an existing private
  absolute directory and any owner record before listening, rejects symlink path
  components and parent traversal, and retains the opened directory without
  creating or changing data. Browser enrollment and sign-in remain unfinished
  ([#69](https://github.com/devoption/devoption/issues/69)).
- A password-form extractor now rejects ambiguous or malformed input before
  authentication, preserves exact decoded UTF-8 bytes, and bounds encoded bodies
  to 4 KiB with a five-second read deadline. Errors are empty and non-cacheable.
  Only fixture routes use it; enrollment, cookie/CSRF protection, and browser
  authentication endpoints remain unfinished
  ([#67](https://github.com/devoption/devoption/issues/67)).
- One authoritative session slot now connects issued grants to the HTTP validator.
  Activation atomically installs a valid grant without renewing its deadline;
  invalid grants preserve the current session, and logout requires its current
  live credential. Fixtures cover rotation, races, expiry, and sign-in through
  HTTP validation. The preview starts with an empty slot; browser authentication,
  cookie transport, and CSRF integration remain unfinished
  ([#65](https://github.com/devoption/devoption/issues/65)).
- Bounded owner sign-in can now issue a fresh internal session credential only
  after verifying the persisted password. It returns an identity-bound validator
  with a one-hour absolute lifetime alongside the credential, shares existing
  attempt limits, and returns no grant on authentication or randomness failure.
  Only temporary fixtures use it; server session retention, cookie/CSRF transport,
  and browser endpoints remain unfinished
  ([#63](https://github.com/devoption/devoption/issues/63)).
- A shared async owner-authentication service limits setup/sign-in to one worker
  at a time and five admitted attempts per rolling minute. Hashing and storage run
  off the runtime thread; cancellation retains the worker slot until work finishes
  and cannot undo setup publication. Only temporary fixtures use it; HTTP enrollment,
  browser sessions, and CSRF handling remain unfinished
  ([#61](https://github.com/devoption/devoption/issues/61)).
- Owner setup and password sign-in now connect the password verifier to durable
  owner storage. Setup generates its identity and salt internally, cannot replace
  an existing owner, and fails closed on storage or randomness errors. Sign-in
  returns only the persisted identity after verification; it grants no session or
  permissions. Only temporary fixtures use these operations; browser setup/sign-in
  remains unfinished ([#59](https://github.com/devoption/devoption/issues/59)).
- Create-once owner record storage for the Linux MVP and macOS development,
  preserving a stable identity and password hash across restarts. It requires a
  private directory, rejects unsafe/corrupt records, and publishes without replacing
  an existing owner. Directory symlinks are rejected even with trailing separators
  or dot components. Only temporary fixture tests use it; the preview still has no
  owner setup or sign-in flow ([#57](https://github.com/devoption/devoption/issues/57)).
- Sessions now bind a server-assigned identity to their token and return it only
  on successful authentication. The HTTP guard passes that identity to protected
  handlers independently of client-supplied identity/role fields. Fixture tests
  cover distinct identities and invalid, expired, and revoked sessions. This adds
  no accounts, permission grants, sign-in UI, or external identity integration
  ([#55](https://github.com/devoption/devoption/issues/55)).
- A password hashing and verification component for future browser owner setup
  and sign-in, using Argon2id with bounded inputs and an explicit cost profile.
  Only fixture tests use it; no accounts, persistent credentials, setup UI, or
  sign-in endpoints exist yet. Browser sessions and CSRF protection remain open
  under [#15](https://github.com/devoption/devoption/issues/15)
  ([#53](https://github.com/devoption/devoption/issues/53)).
- Session validation for unsafe HTTP requests after Host and Origin checks. One
  Bearer Authorization header must encode the session token as 64 hex characters;
  missing or invalid authorization returns an empty, non-cacheable 401 challenge.
  The preview starts without a session and cannot provision one, so all unsafe
  requests remain denied. Tests exercise fixture acceptance and revocation through
  the router. Bootstrap, complete CSRF protection, and project writes remain open
  under [#15](https://github.com/devoption/devoption/issues/15)
  ([#51](https://github.com/devoption/devoption/issues/51)).
- An HTTP-independent session validation core in the existing CLI package, with
  constant-time comparison of 32-byte opaque tokens, a positive lifetime capped
  at one hour, absolute monotonic expiry, and explicit revocation. Deterministic
  tests cover invalid tokens and lifecycle boundaries. Only test fixtures create
  sessions; the preview does not issue or accept them. HTTP integration, authorized
  bootstrap, and CSRF protection remain open under
  [#15](https://github.com/devoption/devoption/issues/15)
  ([#49](https://github.com/devoption/devoption/issues/49)).
- Same-origin validation before routing for methods other than GET, HEAD, OPTIONS,
  and TRACE. Exactly one HTTP Origin must match the validated request host and
  bound port; the two local aliases remain distinct origins. Missing, null,
  duplicate, malformed, or different origins receive empty 403 responses without
  Referer/proxy-header fallbacks or CORS permissions. Added adversarial HTTP tests;
  no write endpoints or session authorization are implemented. This is not a
  complete CSRF solution; [#15](https://github.com/devoption/devoption/issues/15)
  still gates mutations and execution
  ([#47](https://github.com/devoption/devoption/issues/47)).
- Local HTTP authority validation: requests need exactly one `Host` naming
  `127.0.0.1` or case-insensitive `localhost` at the actual bound port, including
  HTTP/1.0 requests. Omitted ports mean 80. Absolute HTTP targets must match;
  invalid, duplicate, nonlocal, or conflicting authorities receive an empty 400
  response. Proxy headers are ignored. Added real HTTP rejection and compatibility
  tests. This is not authentication or CSRF protection; the remaining safeguards
  in [#15](https://github.com/devoption/devoption/issues/15) are still needed
  ([#24](https://github.com/devoption/devoption/issues/24)).
- Tokyo Night in the existing Appearance selector, using upstream Tokyo Night
  and Tokyo Night Light colors adapted to this UI. Nord remains the default;
  mode, preset/custom accent, and reset behavior are preserved. Extended contrast,
  persistence, invalid-theme, storage, and System tests, refreshed page captures,
  and added dark/light Tokyo Night screenshots
  ([#42](https://github.com/devoption/devoption/issues/42)).
- Nord and Dracula theme selection in Appearance, saved independently of mode
  and accent. Theme changes preserve preset names or exact custom colors; both
  themes support Light/Dark/System, with Dracula light identified as a DevOption
  adaptation. Added theme persistence, legacy/invalid-value, storage, reset,
  System, and contrast coverage; refreshed screenshots and palette documentation
  ([#39](https://github.com/devoption/devoption/issues/39)).
- A custom primary color picker on Appearance, saved as one browser-local hex
  value and applied to shared accents and the logo's `dev` lettering. Presets and
  Reset to Purple clear the override while preserving mode. A measured contrast
  warning follows the current palette; controls and focus retain built-in colors.
  Tests cover invalid colors, storage faults, history, reset, and contrast limits;
  refreshed page screenshots include custom-color evidence
  ([#37](https://github.com/devoption/devoption/issues/37)).
- A System mode in Appearance that follows live device color-scheme changes
  while preserving the selected accent. Explicit choices and the dark-purple
  default stay unchanged; tests cover saved System, device changes, and storage
  failure. Refreshed Appearance evidence and recaptured the default pages
  ([#33](https://github.com/devoption/devoption/issues/33)).
- Purple, Blue, and Green primary accents inside Appearance, saved independently
  of light/dark mode with Purple as the default. Shared colors and the logo's
  `dev` lettering follow the accent; tests cover all six combinations, persistence,
  invalid values, and storage failures. Refreshed the Appearance capture and
  recaptured the unchanged default pages
  ([#31](https://github.com/devoption/devoption/issues/31)).
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

- Simplified the README to an early-stage overview, quickstart, four main-page
  screenshots, contributor checks, and project links. Removed the theme gallery
  and implementation catalog while retaining the tracked screenshots and capture
  policy ([#44](https://github.com/devoption/devoption/issues/44)).
- Moved existing Appearance controls to `/settings/appearance`, with a compact
  link in the profile popup and the same saved preferences. Preferences also
  refresh when browser history restores a page. Added route/history tests and
  dedicated-page screenshots; recaptured the default main pages
  ([#35](https://github.com/devoption/devoption/issues/35)).
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
