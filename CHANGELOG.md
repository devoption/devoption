# Changelog

Changes are recorded here before release. No release has been published.
Entries describe implemented changes, not the roadmap.

## [Unreleased]

### Fixed

- Server catalog access shares the owner store's selected directory from startup,
  retaining fresh snapshots and short-lived writer locks throughout the service.
  Temporary fixtures cover storage identity, authenticated operations, startup
  consistency and fail-closed validation
  ([#129](https://github.com/devoption/devoption/issues/129)).

- Copied-binary installation fixtures retry Linux's transient executable-busy
  launch error within a fixed bound, retaining immediate failures for other
  errors and covering held writable descriptors and persistent failures
  ([#118](https://github.com/devoption/devoption/issues/118)).

- Restored Projects pages clear temporary session-check errors after confirmed
  recovery, preserve attachment feedback, and replace interrupted progress with
  a reload warning ([#111](https://github.com/devoption/devoption/issues/111)).

- Repository attachment now distinguishes an unavailable storage response from
  network or unexpected failures while retaining the uncertain-save warning
  and entered path ([#111](https://github.com/devoption/devoption/issues/111)).

- Dropping a catalog writer now explicitly releases its advisory lock, avoiding
  a spurious busy result while a concurrent subprocess retains a descriptor
  before exec. A deterministic regression preserves live-writer exclusion
  ([#113](https://github.com/devoption/devoption/issues/113)).

- Failure to re-inspect an initially valid checkout now returns a stale-checkout
  conflict. Deterministic fixture tests cover disappearance, invalid/unsupported
  metadata and replacement between inspections
  ([#109](https://github.com/devoption/devoption/issues/109)).

- Repository attachment now returns the documented conflict response when the
  configured root changes while serving. Malformed project path segments use
  fixed, non-cacheable errors after authentication; fixture regressions verify
  that both failures preserve the catalog
  ([#109](https://github.com/devoption/devoption/issues/109)).

- The README architecture summary now distinguishes startup catalog validation
  from per-request Projects reads and locked project creation
  ([#105](https://github.com/devoption/devoption/issues/105)).

- CLI help and authentication module introductions now describe CLI-first owner
  setup followed by the implemented browser sign-in flow, while keeping browser
  owner setup explicitly unavailable
  ([#98](https://github.com/devoption/devoption/issues/98)).

- The HTTP mutation guard now requires the browser session cookie and explicit
  CSRF header after Host/Origin validation, rejecting Authorization credentials
  and mixed transports without advertising a Bearer challenge. Transport-neutral
  session APIs remain unchanged; no API or agent authentication is introduced
  ([#96](https://github.com/devoption/devoption/issues/96)).

### Added

- A fixture-only Unix artifact handoff copies fully accounted source hard links
  into a bounded, independent private file before applying the unchanged
  single-link metadata gate and hashing. Disposable tests cover containment,
  replacements, copy/hash-time changes, size bounds and cleanup conflicts. No VM build,
  artifact execution/export or production runner integration is added
  ([#131](https://github.com/devoption/devoption/issues/131)).

- One separately approved diagnostic repeat identifies a regular 445,888-byte
  output with two hard links as the sole artifact rejection. Compilation and
  isolation checks passed; the unchanged gate rejected the output before hashing.
  Cleanup and VM shutdown are verified, with no execution or export of the artifact
  ([#131](https://github.com/devoption/devoption/issues/131)).

- A separately approved disposable Ubuntu rehearsal records one successful offline
  Cargo compile, observed job restrictions, and cleanup. Artifact metadata
  verification failed before a checksum or size was recorded, so the rehearsal
  is not a successful end-to-end build result. A tested, I/O-free helper reports
  each existing metadata predicate without changing acceptance or isolation;
  it is not a production runner
  ([#131](https://github.com/devoption/devoption/issues/131)).

- In-memory build jobs bind queued/running/succeeded/failed/cancelled records to
  an exact immutable plan and attempt identity. Transition tests cover mismatched
  results, cancellation, terminal rewrites and artifact metadata; sealed runner
  events have no production issuer. No process launch, persistence, runtime/UI
  integration or execution authority is added. The accompanying disposable fixture
  runner profile is separate from the job model; its later approved attempts and
  limitations are recorded under #131 above
  ([#127](https://github.com/devoption/devoption/issues/127)).

- A single-crate local seed provider verifies and supplies actual archive bytes.
  A separate trusted Linux CI fixture compiles against the pinned dependency
  through Cargo source replacement, records an artifact checksum, and rejects
  missing/corrupt seeds without a fallback. Production approval, isolation, job
  execution and deployment remain unavailable
  ([#125](https://github.com/devoption/devoption/issues/125)).

- A fixture-only root Cargo source verifier checks typed Git commit/tree/blob
  identities with SHA-1 collision detection, rejects ambiguous root paths/types
  and unsupported configuration, then validates linked manifest/lockfile bytes.
  Independent revision selection, complete source/context, plan binding, archive
  verification and execution remain unimplemented
  ([#123](https://github.com/devoption/devoption/issues/123)).

- A bounded supplied-byte Cargo adapter checks a strict standalone manifest and
  version-4 lock graph using pinned TOML/SHA-256 dependencies. Exact manifest and
  lockfile hashes now participate in provider matching and build-plan equality;
  synthetic fixtures cover unsupported, malformed, missing, ambiguous and mismatched
  inputs. Verified commit provenance, actual dependency-byte integrity, resolution
  and execution remain unimplemented
  ([#121](https://github.com/devoption/devoption/issues/121)).

- A separate Cargo registry contract records immutable locked dependency,
  original-source, checksum and materialization metadata. Its checked provider
  handoff rejects mismatched requests without a public-fetch fallback, preserves
  Cargo package-name case while rejecting case-colliding coordinates, and build
  plans include every descriptor field in equality. Only fixture providers exist;
  registry hosting, actual fetching, approval and execution remain unimplemented
  ([#119](https://github.com/devoption/devoption/issues/119)).

- A separate Unix build-plan domain validates immutable source, toolchain,
  command, path, environment and limit metadata against a catalog snapshot.
  Fixed Cargo arguments and bounded fields have fixture coverage; approval,
  execution and runtime integration remain unimplemented
  ([#116](https://github.com/devoption/devoption/issues/116)).

- Catalog-inclusive offline backup/restore instructions and installed-binary
  fixtures cover version-2 repository associations and default checkouts, missing
  catalogs, rejected old sessions and invalid restores. Only temporary fixture
  data is copied; repository contents and root relocation remain outside this
  application-state backup ([#114](https://github.com/devoption/devoption/issues/114)).

- Per-project **Attach repository** forms accept bounded relative checkout paths
  when the server has an explicit trusted root. Projects shows saved repository
  names and default checkout paths, reloads after confirmed saves, and explains
  duplicate, stale, unavailable and uncertain results without exposing private
  errors. Fixture HTTP, script and browser checks cover the flow; the Projects
  screenshot is refreshed. No real root is configured or Git command executed
  ([#111](https://github.com/devoption/devoption/issues/111)).

- An explicit Unix `serve --repository-root` option and owner-protected backend
  endpoint attach validated local checkouts to projects. Linked worktrees share
  one repository identity, with a default checkout per project/repository
  association. Locked atomic version-2 storage preserves version-1 metadata and
  rejects conflicting roots or stale bindings. Fixture checks cover migration,
  restart, concurrency, authorization and failure paths. Browser intake and
  execution remain deferred; no real root is configured
  ([#109](https://github.com/devoption/devoption/issues/109)).

- A read-only Unix worktree-inspection core under an explicit trusted local root.
  Bounded metadata reads recognize a conservative ordinary/linked-worktree subset,
  reject unsafe or out-of-root paths and return distinct worktree/common-directory
  filesystem identities. Fixture tests cover supported layouts and rejection paths.
  No Git commands, browser intake, catalog migration or production registration
  are enabled ([#107](https://github.com/devoption/devoption/issues/107)).

- Signed-in owners can create named empty projects from the Projects form. Names
  are bounded and escaped, writes use the catalog lock and atomic persistence,
  and successful saves reload the current list without restarting the server.
  Cookie/CSRF/Origin guards reject unauthorized or mixed-transport requests;
  corruption, contention and write failures never report successful creation.
  Concurrency, restart, input and browser checks cover the new flow. The screenshot
  uses temporary fixture data; no repository paths or Git operations are added
  ([#105](https://github.com/devoption/devoption/issues/105)).

- Configured Projects pages now show saved project names and repository counts
  behind the existing session guard. A bounded domain reader loads a validated
  startup snapshot without writing files; invalid catalogs stop startup, absent
  catalogs show an empty list, and unconfigured previews retain their placeholder.
  Names are escaped, restart/denial/failure paths are tested, and the Projects
  screenshot uses temporary fixture data. No editing, path intake or Git execution
  is exposed ([#103](https://github.com/devoption/devoption/issues/103)).

- A dedicated `devoption-projects` crate with separate stable project/repository
  identities, multi-repository associations and a bounded versioned private Unix
  catalog. Exclusive writer locking and atomic snapshots preserve complete state
  across restart; failure tests cover partial writes, invalid files and conflicts.
  Repository records are identity/display metadata only: path intake, Git
  validation and HTTP/UI registration remain unimplemented
  ([#101](https://github.com/devoption/devoption/issues/101)).

- Optional, explicitly approved Linux user lingering guidance and a recorded
  disposable Ubuntu rehearsal proving readiness before the first login after
  reboot, continued operation after logout, and deliberate stop behavior. The
  temporary probe was removed and the original `Linger=no` policy restored
  ([#94](https://github.com/devoption/devoption/issues/94)).

- A recorded disposable Ubuntu 24.04 x86_64 source-install rehearsal covering
  SSH browser authentication, the user service, private backup/restore, old-session
  rejection, complete logout and recovery after reboot and login. VM artifacts
  stay in one ignored repository folder with a monitored disk budget and explicit
  stop/cleanup instructions; platform and unattended-service limits remain stated
  ([#92](https://github.com/devoption/devoption/issues/92)).

- Private Ubuntu source-install instructions, an unprivileged systemd user-unit
  example with an owner-record preflight, same-port SSH forwarding, and stopped
  backup/restore guidance. Fresh installation preflights the physical home path,
  parent directories and both destinations, rejecting symlinks before creating
  directories. The guide distinguishes enabling a unit from starting it with
  `--now`. Release fixtures check copied-binary startup, restored authentication,
  lost sessions and damaged records.
  The damaged-record check holds an occupied port so an accepted record fails the
  assertion instead of leaving a server running indefinitely.
  Ubuntu CI statically checks the unit. Real systemd/SSH lifecycle validation
  remains pending
  ([#90](https://github.com/devoption/devoption/issues/90)).

- Session checks for configured Home, Projects, About and Appearance GET/HEAD
  reads, with fixed non-cacheable redirects to Sign in and no protected content
  on rejection. The strict browser cookie is required; page reads never renew
  sessions or weaken mutation CSRF. Sign in and the unconfigured preview stay
  public. Fixture tests cover expiry, rotation, logout, restart and loss of the
  owner record after startup. Screenshot instructions use the temporary configured
  fixture for every capture and specify each required sign-in state
  ([#88](https://github.com/devoption/devoption/issues/88)).
- Browser Sign in and Account screens for an existing CLI-created owner, with
  shared profile sign-out controls, labelled keyboard/mobile forms, fixed errors,
  password clearing, and memory-only CSRF tokens. Same-origin session recovery
  handles reloads without renewing or rotating sessions. Fixture tests cover
  cookie/Origin rejection, stale logout, request failures, and navigation races;
  failed session checks offer an explicit account recheck without another password
  submission, including when sign-in succeeded before verification failed.
  Real-browser captures document the current pages. CLI-only owner creation and
  public preview pages are unchanged ([#86](https://github.com/devoption/devoption/issues/86)).
- Explicit local/SSH first-owner setup with hidden, bounded password confirmation,
  terminal cancellation/restoration, server-owned enrollment policy, and existing
  private create-once storage. Temporary PTY fixtures cover input and failure paths.
  After terminal restoration, cancellation signals exit immediately so blocking
  hashing/storage cannot silently ignore them; subprocess fixtures cover this phase.
  Browser installation is deferred; sign-in forms remain unfinished
  ([#84](https://github.com/devoption/devoption/issues/84)).
- A locally armed enrollment authorization core, bound to one owner service/store.
  It exchanges a single-use launch credential for an independent browser pair,
  enforces one five-minute deadline and create-once enrollment, and retains bounded
  secret-free audit evidence including completion after caller cancellation.
  Only temporary fixtures use it; browser handoff, browser setup routes/forms,
  enrollment password policy, and durable audit remain unfinished
  ([#81](https://github.com/devoption/devoption/issues/81)).
- Existing-owner HTTP challenge, sign-in, and sign-out routes in the server crate,
  enabled only when explicit data-directory startup validation finds an owner.
  They enforce Host/Origin, consume pre-authentication CSRF before password input,
  share bounded authentication work and the active session, and return fixed
  non-cacheable failures. Full challenge capacity includes a bounded `Retry-After`
  based on the earliest pending expiry. Temporary CLI/HTTP fixtures cover real
  routing, rotation, logout, attempt limits, and store failures. First-owner
  enrollment and browser forms remain unavailable ([#79](https://github.com/devoption/devoption/issues/79)).
- Single-use pre-authentication CSRF challenges in the server crate, with a
  separate Secure, HttpOnly, SameSite=Strict cookie and explicit synchronizer
  header. Storage holds at most 16 pending pairs for five minutes; consumption
  is atomic and precedes fixture password processing. Host/Origin guards are
  shared without changing production authorization. Enrollment authorization
  and production setup/sign-in endpoints remain unfinished
  ([#77](https://github.com/devoption/devoption/issues/77)).
- Browser cookie transport now connects the HTTP mutation guard to session-bound
  CSRF validation. Fixture-only activation/logout helpers issue and clear a
  host-only Secure, HttpOnly, SameSite=Strict cookie; mixed or ambiguous credentials
  fail closed. Pre-authentication CSRF, enrollment authorization, and browser
  endpoints remain unfinished. A [manual fixture check](docs/browser-session-check.md)
  documents browser verification ([#73](https://github.com/devoption/devoption/issues/73)).
- Issued owner sessions now bind a separate random synchronizer CSRF token.
  Browser validation and logout require the matching live token pair in the
  shared session slot; rotation and expiry invalidate both. This is fixture-tested
  core support; cookie transport, pre-authentication CSRF, enrollment authorization,
  and browser endpoints remain unfinished
  ([#71](https://github.com/devoption/devoption/issues/71)).
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

- Extracted implemented HTTP/runtime, authentication, session, and owner-storage
  responsibilities into `devoption-server`. The CLI now handles arguments and
  service startup, with dependencies flowing CLI → server → UI. Existing behavior,
  commands, and tests are retained; no authentication feature was added
  ([#75](https://github.com/devoption/devoption/issues/75)).
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
