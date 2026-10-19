# Changelog

Changes are recorded here before release. No release has been published.
Entries describe implemented changes, not the roadmap.

## [Unreleased]

### Changed

- Record the first approved live bundle-signing run, GitHub attestation and
  successful cryptographic verification, with the exact source/archive digest
  and remaining retention/publication limits
  ([#261](https://github.com/devoption/devoption/issues/261)).

### Added

- Retain verified signed candidates for seven days under distinct run/attempt
  artifact IDs without replacement, then reverify downloaded archive/proof bytes
  in a separate read-only job; preserve live activation and publication gates
  ([#261](https://github.com/devoption/devoption/issues/261)).

- Prepare a main-only opt-in workflow that builds, tests and attests one bundle,
  plus a pinned cryptographic verifier enforcing reviewed source/digest, workflow
  identity, main ref, hosted runners and archive integrity; keep live signing
  and attestation writes subject to separate approval
  ([#261](https://github.com/devoption/devoption/issues/261)).

- Include a deterministic CycloneDX source-inventory SBOM, vendored Highlight.js
  notices and digest-bound unsigned assembly provenance in local bundle
  candidates; validate dependency identities, checksums and sidecars while
  retaining incomplete coverage, authentication and publication gates
  ([#261](https://github.com/devoption/devoption/issues/261)).

- Assemble repeatable local Ubuntu 24.04 amd64 preview bundle candidates with
  a versioned file/compatibility/dependency manifest, checksums and packaged
  executable smoke checks; reject mismatched, dirty or replaceable inputs while
  leaving SBOM, authenticated provenance, installer and publication work gated
  ([#261](https://github.com/devoption/devoption/issues/261)).

## [0.1.0-preview.1] - 2026-10-18

### Changed

- Use the checksum-pinned GNU/Linux release-plz build to avoid musl changelog
  parser stack-limit failures; run its fixtures in ordinary read-only Ubuntu PR
  CI and retain bounded subprocess failure diagnostics
  ([#260](https://github.com/devoption/devoption/issues/260)).

- Add an opt-in main-only draft release PR writer using the short-lived workflow
  token after source CI and metadata validation; restrict the immutable candidate
  to root version/lockfile/changelog changes, recover identical interrupted
  creations, and preserve required PR checks and human merge/publication gates
  ([#260](https://github.com/devoption/devoption/issues/260)).

- Require version-bound release metadata for preview proposals and validate its
  supported platform, compatibility, migration and recovery limits; append the
  checked statements to proposed notes and reject missing, stale or edited
  metadata blocks in the read-only release check without enabling publication
  ([#260](https://github.com/devoption/devoption/issues/260)).

- Select automatic preview versions from complete Git/tag baselines and
  repository-wide shipped changes, including helpers and service templates;
  preserve human notes/history, validate numbered preview notes, and reject
  missing/stale baselines, collisions and unrelated dependency changes while
  retaining the read-only preparation/publication boundary
  ([#260](https://github.com/devoption/devoption/issues/260)).

- Add a pinned release-plz configuration, disposable version/changelog proposals
  and a read-only manual workflow gated by CI; exercise stable-version bumps,
  no-release cases, retry/failure behavior and current preview limitations without
  enabling release PR creation, tags or publication
  ([#260](https://github.com/devoption/devoption/issues/260)).

- Validate deployment port collisions, invocation identity and uncertain cleanup
  on disposable Ubuntu with a repeatable opt-in checker; reject absent or malformed
  invocation IDs in worker health and HTTPS route verification
  ([#19](https://github.com/devoption/devoption/issues/19)).

- Reconcile completed single-node/public-host acceptance and the superseded native
  experiment, retaining #19's unproven deployment integration cases; refresh
  security reporting and repository settings from read-only GitHub evidence,
  including the distinction between workflow permissions and repository defaults
  ([#256](https://github.com/devoption/devoption/issues/256)).

- Record fresh source-built Ubuntu cloud acceptance for two public HTTPS
  hostnames, ACME issuance/accelerated renewal, real A/B builds, failed health,
  saved-release recovery, running/stopped reboots, snapshot restore and
  data-preserving reinstall; verify removal of the temporary node and only its
  added DNS records, and link current capability summaries to the evidence
  ([#234](https://github.com/devoption/devoption/issues/234)).

- Raise the external HTTPS checker's bounded response limit to 128 KiB so the
  current 90,219-byte sign-in page passes; retain oversized-response rejection
  ([#234](https://github.com/devoption/devoption/issues/234)).

- Reconcile the roadmap, architecture and accepted-decision index with implemented
  trusted Wasm builds, immutable releases, same-node deployment and local lifecycle
  evidence; preserve outstanding fresh-host/public DNS/ACME acceptance and broader
  platform limits, and make the operational guides easier to find
  ([#253](https://github.com/devoption/devoption/issues/253)).

- Add a credential-free external HTTPS verification command and acceptance-report
  instructions for both single-node hostnames, with exact DNS/address checks,
  trusted TLS, app health and control-route separation checks; public-host
  acceptance remains pending
  ([#234](https://github.com/devoption/devoption/issues/234)).

- Add a persistent folder tree, line-numbered syntax highlighting, and default
  tracked root README previews with a Preview/Source switch. Markdown renders
  without active repository HTML or remote images, under existing read bounds
  ([#250](https://github.com/devoption/devoption/issues/250)).

- Make project overviews easier to scan with repository rows, expandable checkout
  locations, a guided empty state, and links to code, builds and releases
  ([#248](https://github.com/devoption/devoption/issues/248)).

- Allow bounded deployment approvals to finish through the initial platform proxy,
  correct the Overview capability description, and record a clean local VM
  rehearsal of browser builds/deployments, two hostnames, updates, reboot,
  snapshot recovery and data-preserving removal/reinstallation; public DNS/ACME
  acceptance remains pending
  ([#234](https://github.com/devoption/devoption/issues/234)).

- Add bounded offline snapshots and verified, non-overwriting restore staging for
  owner records, build history/artifacts and saved app releases on the same
  installation; require stopped workers/intent and retain explicit external-input
  and disaster-recovery limits
  ([#234](https://github.com/devoption/devoption/issues/234)).

- Run Rust lint, tests, release installation checks, and script checks in parallel;
  avoid rerunning code checks when a draft PR becomes ready for review or its
  description is edited, while retaining separate contribution validation
  ([#244](https://github.com/devoption/devoption/issues/244)).

- Add a staged platform service generator that aligns installed build/deployment
  storage and opt-ins, plus one operator path from installation to app updates,
  with explicit outstanding recovery and cloud acceptance checks
  ([#234](https://github.com/devoption/devoption/issues/234)).

- Bind a second HTTPS hostname to one project, generate reviewed Caddy and runtime
  configuration, and show verified local routing/certificate status in Releases
  ([#233](https://github.com/devoption/devoption/issues/233)).

- Save verified Wasm builds as immutable local releases and deploy, inspect,
  stop, update and recover the selected app through the project interface using
  an opt-in bounded private Wasmtime worker
  ([#232](https://github.com/devoption/devoption/issues/232)).

- Build the selected trusted Wasm web app through project review and explicit
  approval using an opt-in Linux systemd worker, prepared offline inputs, bounded
  logs, cancellation and durable artifact provenance. Add the real fixture and
  installation guide
  ([#231](https://github.com/devoption/devoption/issues/231)).

- Admit the selected web-app's narrow `cdylib` manifest and complete library source
  snapshot, with Cargo-compatible exact dependency matching that retains full
  locked version/checksum identity. Reject mixed targets and keep library snapshots
  out of the native-only build planner; Wasm execution and UI integration remain
  pending ([#231](https://github.com/devoption/devoption/issues/231)).

- Add explicit HTTPS public-origin serving behind a local proxy, requiring an
  existing owner and strict Host/Origin checks while retaining loopback binding,
  Secure host-only cookies and CSRF protection. Add dedicated systemd/Caddy
  installation examples and disposable TLS/login/project/restart checks; local Ubuntu
  system-service HTTPS, state preservation and startup before login after reboot
  are verified; actual cloud DNS, ACME and fresh-host validation remain pending
  ([#230](https://github.com/devoption/devoption/issues/230)).

- Select the first Wasm web-app implementation contract: WASIp2 HTTP components,
  a supervised Wasmtime worker, explicit limits and a bounded fixture validation
  plan; map native build assumptions to required integration work without adding
  runtime support ([#229](https://github.com/devoption/devoption/issues/229)).

- Prioritize a six-task single-node delivery track from a fresh VM and authenticated
  platform DNS to a deployed web app on its own HTTPS hostname, with update/restart
  acceptance and explicit deferral of broader platform work; this is planning,
  not newly available hosting behavior
  ([#228](https://github.com/devoption/devoption/issues/228)).

- Show authenticated project build history and attempt details with saved outcomes,
  expandable source identities, escaped stdout/stderr and truncation/recovery states;
  scope fresh reads to current catalog associations while execution remains unavailable
  ([#226](https://github.com/devoption/devoption/issues/226)).

- Validate all saved build-history project/repository associations under the catalog
  lock before startup recovery; reject foreign history without rewriting it
  ([#224](https://github.com/devoption/devoption/issues/224)).

- Recover explicitly selected private build history before serving and retain its
  exclusive writer lock for the service lifetime; builds remain unavailable
  ([#222](https://github.com/devoption/devoption/issues/222)).

- Persist bounded output checkpoints for known build attempts with private atomic
  snapshots, append-only stream checks and preserved escaped rendering after restart
  ([#220](https://github.com/devoption/devoption/issues/220)).

- Allocate random build-attempt identities and durably save Queued before returning
  a job; reject retained-ID collisions and unknown history updates
  ([#218](https://github.com/devoption/devoption/issues/218)).

- Persist up to ten private build-job summaries with exclusive writer locking,
  bounded checksummed snapshots, atomic replacement and explicit interrupted
  recovery; no restored execution authority or application route
  ([#216](https://github.com/devoption/devoption/issues/216)).

- Test Linux orphan-descendant cleanup through an isolated subreaper, retaining
  the exited leader identity until signaling ends and verifying both child waits
  and pipe closure; no production runner
  ([#213](https://github.com/devoption/devoption/issues/213)).

- Exercise build supervision against real trusted leaf subprocesses, including
  nonblocking output, timeout/cancellation, cap-triggered termination and TERM-to-KILL
  escalation; keep production execution disabled
  ([#211](https://github.com/devoption/devoption/issues/211)).

- Add terminal Interrupted build records and a recovery constructor that cannot
  restore running state, success or approval; document remaining build integration
  ([#208](https://github.com/devoption/devoption/issues/208)).

- Add monotonic build supervision decisions for timeout, cancellation and bounded
  output, with sticky stop reasons and a five-second termination grace; no process
  launch, signaling or cleanup claims
  ([#204](https://github.com/devoption/devoption/issues/204)).

- Add an in-memory build-output collector with a combined stdout/stderr byte cap,
  sticky truncation and escaped text rendering; no process readers or execution
  ([#202](https://github.com/devoption/devoption/issues/202)).

- Add bounded read-only SHA-256 verification and retained handles for reviewed
  Cargo/rustc files, with no-follow traversal and mutation/replacement rechecks;
  no tool launch or approval integration
  ([#200](https://github.com/devoption/devoption/issues/200)).

- Add read-only verification and retained handles for reviewed private job-directory
  identities, with no-follow traversal and replacement/permission rechecks; no
  directory creation, cleanup or process execution
  ([#198](https://github.com/devoption/devoption/issues/198)).

- Enforce the initial explicit environment allowlist, reviewed path bindings and
  proposed time/log caps when validating build review contexts and issuing approval;
  no inherited environment or process execution
  ([#196](https://github.com/devoption/devoption/issues/196)).

- Bind approval records to a bounded typed review context with deterministic,
  versioned hashing and plan/path/identity checks; retain the distinction between
  supplied metadata and verified execution state
  ([#194](https://github.com/devoption/devoption/issues/194)).

- Add bounded in-memory build approval records with exact owner/job/plan/context
  binding, five-minute expiry, single consumption, revocation and restart-invalid
  handles; no production approval adapter or process execution
  ([#192](https://github.com/devoption/devoption/issues/192)).

- Define the local mutation and future execution trust contract, including explicit
  approval invalidation, environment permissions, owned storage, resource/output
  limits and the review gate; distinguish existing HTTP protections from dependent
  runner work ([#15](https://github.com/devoption/devoption/issues/15)).

- Add a concise shared-context handbook for scoped engineering tasks, with product
  context, architecture, accepted decisions and a portable task/handoff template;
  link existing guides rather than duplicate them
  ([#189](https://github.com/devoption/devoption/issues/189)).

- Document the platform roadmap, separating current capabilities from planned
  Rust-to-Wasm builds, deployment, optional MCP and later team/platform features;
  link it from the README ([#183](https://github.com/devoption/devoption/issues/183)).
  
- Improve `ROADMAP.md` clarity and structure: lead with
  audience/why, link the MVP phase to its milestone, split the MVP into numbered
  stages with clear “done when” criteria aligned to acceptance checks, add a brief
  Rust-to-Wasm vs containers note, and include a short “How to help” section
  (docs-only) ([#188](https://github.com/devoption/devoption/issues/188)).

- Scope newly created repository storage and names to each project, allowing each
  project to have its own `ui` while preserving legacy paths and stable locations
  after project renames ([#181](https://github.com/devoption/devoption/issues/181)).

- Open the tracked-file browser immediately in project Code, retain repository
  selection and place status/attachment actions below files. New repositories open
  their own empty file list. Align shared desktop breadcrumbs with the sidebar
  logo, preserve mobile spacing and refresh the affected page captures
  ([#179](https://github.com/devoption/devoption/issues/179)).

- Capture complete immutable source snapshots from an explicitly selected saved
  repository and current commit. Collect bounded Git objects, verify their full
  source graph and compare exact checkout bytes, including changes hidden by Git
  normalization or index flags. Keep this backend API separate from approval,
  extraction and execution
  ([#177](https://github.com/devoption/devoption/issues/177)).

- Require complete immutable source snapshots in candidate build plans. Verify a
  bounded commit/tree/blob graph, retain every supported source file's exact bytes,
  and match the conventional binary target to the verified package. Reject missing,
  extra or inconsistent objects without reading live repositories or enabling
  extraction, approval or execution
  ([#175](https://github.com/devoption/devoption/issues/175)).

- Add local/SSH `devoption check-source` with explicit catalog/root paths and saved
  project/repository IDs. Reuse read-only source preparation, print observed facts
  and check outcomes, and document passed/failed/invalid-argument exit statuses
  without HTTP, owner-credential reads, source edits or execution approval
  ([#173](https://github.com/devoption/devoption/issues/173)).

- Replace the project Builds placeholder with real read-only source preparation:
  inspect observed HEAD, Git changes, tracked Cargo inputs and conventional binary
  source. Show actionable results and fresh rechecks without creating a build job
  or implying execution approval; refresh Builds captures and document the limits
  ([#171](https://github.com/devoption/devoption/issues/171)).

- Add **Browse files** to project Code with authenticated tracked-file lists and
  escaped, bounded working-tree text previews. Support ordinary links, keyboard
  navigation and browsing without JavaScript; explain empty, binary, oversized
  and unavailable files without exposing untracked contents or enabling execution
  ([#169](https://github.com/devoption/devoption/issues/169)).

- Bind candidate build plans to verified root Cargo source proofs. Reject commit
  and dependency-request mismatches and retain proof identity in job plan equality;
  build execution and approval remain unimplemented
  ([#167](https://github.com/devoption/devoption/issues/167)).

- Add **New repository** on a project's Code page with explicit managed-root
  startup configuration. Create and persist empty private Git repositories,
  preserve existing paths, and keep inspection-only roots unchanged. Document
  interrupted creation and recovery without automatic deletion or retry
  ([#162](https://github.com/devoption/devoption/issues/162)).

- Add a dedicated Profile page for saving or clearing a display name. Keep names
  in private server storage across restarts and show the name and initials in the
  account menu, with “Account” when unset and Owner retained as the role. Include
  profile metadata in the documented offline backup/restore procedure
  ([#161](https://github.com/devoption/devoption/issues/161)).

- Open projects in a dedicated workspace with Overview, Code, Builds, Releases,
  Packages and Settings navigation. Repository inspection stays in Code and
  renaming moves to Settings; unavailable features retain explicit states.
  Section URLs support refresh, deep links and browser history
  ([#160](https://github.com/devoption/devoption/issues/160)).

- Shorten the README, move usage and developer detail into linked guides, and keep
  the full screenshot gallery in documentation. Clarify the intended individual
  and team audience in docs and UI, remove the About page and development badge,
  and preserve current preview limits
  ([#157](https://github.com/devoption/devoption/issues/157)).

- Give sign-in, account recovery and CLI owner-setup guidance a dedicated branded,
  responsive page using the saved browser theme. Preserve the cookie/CSRF flow,
  clarify pending and ended-session feedback, and retain accessible password
  entry and keyboard retry ([#155](https://github.com/devoption/devoption/issues/155)).

- Apply the built interface's shared sidebar, utility header, page hierarchy,
  project rows and control styling across the app. Project creation, rename and
  repository attachment use keyboard-accessible dialogs; status reads remain
  inline. Appearance uses direct choices on its dedicated page while retaining
  existing preferences and themes. Releases and Infrastructure explicitly show
  unavailable states, without execution or deployment controls
  ([#153](https://github.com/devoption/devoption/issues/153)).

### Fixed

- Add read-only review and explicit, backed-up local recovery for changed repository
  device bindings. Revalidate every registered layout and reject stale approvals,
  replaced/unsafe entries and data overlap; retain normal fail-closed startup
  ([#210](https://github.com/devoption/devoption/issues/210)).

- Retry bounded profile reads when a concurrent atomic save retires the opened
  record before validation, preserving private-file and identity checks instead
  of reporting a valid replacement as corrupt. Cover the race deterministically
  and retain concurrent-save coverage
  ([#185](https://github.com/devoption/devoption/issues/185)).

- Successful sign-in now opens Overview automatically after confirming the session
  cookie, without requiring an Open workspace click. Failed or unverified attempts
  stay on the authentication page
  ([#159](https://github.com/devoption/devoption/issues/159)).

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

- Successful repository status reads show their browser-local check time after
  response validation; loading, failed reads, and discarded history responses
  cannot display a fresh successful-check time
  ([#151](https://github.com/devoption/devoption/issues/151)).

- The Projects list filters saved project names in the browser, with match counts,
  no-results feedback and a clear action that preserves existing row/form state
  ([#145](https://github.com/devoption/devoption/issues/145)).

- Owners can rename saved projects from their details page, retaining stable
  identities, repository associations and checkout bindings through the existing
  authenticated, atomic catalog write path
  ([#143](https://github.com/devoption/devoption/issues/143)).

- Project details can read an attached checkout's branch, commit and working-tree
  counts on demand, with refresh, detached/unborn labels and explicit unavailable
  states. Controlled Git status uses private temporary metadata, bounded reads,
  and saved-association revalidation without source-index or catalog writes
  ([#141](https://github.com/devoption/devoption/issues/141)).

- Repository attachment offers a bounded picker of supported immediate child
  checkouts, with refresh, loading/empty/error feedback and manual path fallback
  ([#139](https://github.com/devoption/devoption/issues/139)).

- Creating a project opens its saved details directly, including for duplicate
  names. Empty projects expand the configured first-repository attachment form
  ([#137](https://github.com/devoption/devoption/issues/137)).

- Saved project names open a dedicated details page with repositories, relative
  checkout paths, repository attachment and truthful build availability. Attachment
  keeps you on the current page ([#135](https://github.com/devoption/devoption/issues/135)).

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

<!-- devoption-release-metadata:start -->
### Release compatibility and support

- Preview version: `0.1.0-preview.1`; supported platform: Ubuntu 24.04 amd64.
- Installation: fresh only. Upgrades from earlier platform versions are unsupported.
- Data compatibility: Use fresh platform state. Existing installations are not supported upgrade targets for this preview.
- Configuration compatibility: Use the operator-managed source installation instructions in docs/public-linux-install.md; no automatic configuration conversion is provided.
- Migration: none implemented. No platform data or configuration migration is implemented. Do not reuse existing installation state as an upgrade procedure.
- Binary rollback: unsupported. Do not assume replacing the executable reverses data changes. The same-installation snapshot and staged restore in docs/same-node-recovery.md require the original external files and operator verification; they are not cross-version upgrade recovery.
- Support status: development preview.
- Warning: One owner and one trusted stateless Rust/WASIp2 app on a dedicated Ubuntu 24.04 amd64 node; arbitrary applications and hostile multi-tenant hosting are unsupported.
- Warning: Source installation remains operator-managed. Published bundles, a verified bundle installer and supported platform upgrades are not available.
- Warning: Preview metadata is not production-readiness evidence or permission to publish; the release approval gate under issue #21 still applies.
- Metadata SHA-256: `d4c93fe0e7496f6085adc42cca8df62272ba5ba6743cc1fb3cc8a3e0d4aff343` (metadata identity, not artifact provenance).
<!-- devoption-release-metadata:end -->
