# Release preparation dry run

The preparation work under [#260](https://github.com/devoption/devoption/issues/260)
produces a reviewable preview version/changelog proposal. It does not create a
release PR, merge, tag, GitHub release, bundle or registry upload. The existing
[release approval process](releases.md) and #21 remain applicable. There is still
no published product release; the source version remains `0.1.0-dev`.

## Run a proposal

After this workflow is merged, dispatch **Release preparation dry run** from
`main`. It runs the existing full CI workflow first. Only successful checks allow
the proposal job to run, against that run's exact checked-out commit, even if
`main` advances later. The job installs a checksum-pinned release-plz 0.3.172,
fetches locked Cargo inputs and runs the tool's disposable fixtures. Its summary
contains a JSON report and proposed patch. Workflow permissions are read-only,
checkout credentials are not persisted, and no forge token is supplied to the
tool. Concurrent preparation runs queue rather than cancel each other.

For a local run, install that exact tool version from the
[official release](https://github.com/release-plz/release-plz/releases/tag/release-plz-v0.3.172)
and verify the archive before executing it. The workflow pins the Ubuntu x86_64
musl archive to SHA-256
`db58a4668ce9ef0b6431e96a665715745e9a5d7f0b0b28a610034d94de3970df`.
The macOS ARM64 archive used for local verification has SHA-256
`1b2019acd2f218730d85d9426387c872ee830e73261218b6863b6e814dde86b0`.
Both match the official GitHub release asset digests checked on 2026-10-18.
This pins downloaded bytes; it is not build-provenance verification for DevOption
bundles (tracked separately in #261).

From a clean, committed checkout with locked dependencies available locally:

```sh
python3 scripts/release_plan.py \
  --release-plz /absolute/path/to/release-plz \
  --channel preview \
  --date 2026-10-18 \
  --output /tmp/devoption-release-proposal
```

Use the intended proposal date; omitting `--date` uses the local current date.
Choose a new output directory outside the source checkout. The script clones the
exact source commit to temporary storage, selects a preview and runs
`release-plz set-version` with Cargo offline mode there. It checks the resulting
workspace/lockfile versions, unchanged dependency inputs and allowed file changes,
then writes `report.json` and `proposal.patch`. The original checkout, commits and tags
stay unchanged. A failed tool/check produces no successful output directory;
retry with the same source, tags, tool and date produces the same proposal. This
is not a test of partial *publication* recovery: no publication path exists here.

## Version and notes behavior

The [release-plz configuration](https://release-plz.dev/docs/config) uses Git tags
matching `v{{ version }}`, rather than crates.io, as version baselines. All seven
workspace crates inherit one workspace version, remain `publish = false`, and
belong to the same version group. All packages reference the shared root changelog.
Preview proposals move the existing human-written `Unreleased` body into a dated
preview section, keeping historical sections byte-for-byte and leaving an empty
`Unreleased` section. Missing change bullets, issue links, ambiguous sections or a
future date block the proposal. Reviewers still assess note accuracy and coverage.
The proposed section also requires a version-bound
[release metadata record](release-metadata.md). Its support, compatibility,
migration and recovery statements are appended as a checked block; the report
records the exact metadata digest. Missing, stale or inconsistent metadata blocks
a preview proposal before temporary version edits. No-release runs and raw
analysis diagnostics do not claim a metadata-validated candidate.

Preview selection requires full Git history and available platform tags. It
compares the source with the tag matching its embedded workspace version and
records both commit IDs. That baseline must be an ancestor and its manifest must
agree with the tag. Missing baselines, newer known versions or a conflicting
target tag fail closed. An already prepared but untagged preview blocks another
proposal until its publication is completed or explicitly reconciled; it never
silently reuses or skips the version. Tags are trusted repository inputs here,
not independent evidence of publication or artifact authenticity.

| Baseline | Next preview when shipped content changes |
| --- | --- |
| `X.Y.Z-dev`, with no platform tags | `X.Y.Z-preview.1` |
| Tagged `X.Y.Z-preview.N` | `X.Y.Z-preview.(N+1)`; the base stays fixed throughout that preview series |
| Tagged stable `X.Y.Z` | Next patch for fixes/other source changes, next minor for features, next minor for breaking changes in 0.x or next major after 1.0; append `-preview.1` |

A breaking `!` or `BREAKING CHANGE:` footer takes precedence over features. The
report records that classification even within a fixed preview series. Stable
promotion is not available. The version is selected before any future binary
build, and changes to embedded versions will require rebuilding and retesting.

Change detection covers the entire repository, including Python helpers, service
templates, toolchain/lockfiles, deletions and unknown paths. Exclusions are the
named root documentation files, `docs/`, GitHub Markdown templates,
`scripts/test_*` and standalone `crates/*/tests/` files. Other paths conservatively
count as shipped content until a bundle inventory narrows that set. A `docs`
commit that changes source still triggers a patch. Documentation/test-only or
fully reverted net changes produce no patch and leave `Unreleased` notes alone.
Conventional titles remain mandatory; API compatibility is not inferred.

`--channel analysis` retains the original raw `release-plz update` diagnostics.
Only the CLI generates its changelog, including other crates' commit summaries.
That diagnostic mode still misses non-crate changes and leaves prerelease
versions unchanged; its report explicitly retains those limitations.

Run all fixtures with the real pinned binary:

```sh
DEVOPTION_RELEASE_PLZ=/absolute/path/to/release-plz \
  python3 -m unittest discover -s scripts -p 'test_release_*.py'
```

Normal Python CI runs policy/boundary checks and skips the tool-dependent fixtures;
the manual proposal workflow requires them. Fixtures use disposable Git tags and
small local crates, and test version groups, no-release behavior, breaking
changes, preview numbering, helper-only changes, baselines, deterministic retries
and failures.

## Remaining #260 work before enablement

- Generate narrowly scoped bot-owned draft release PRs with issue-linked commits,
  required checks on their exact revision and a reviewed authentication approach.
  No auto-merge or repository-policy change is included here.
- Reconcile #21 with ongoing publication authority, serialize publication with
  preparation, and test duplicate-version, partial-publication and retry recovery
  against immutable artifact identities. Version proposals alone cannot do this.

These are remaining implementation/approval gates, not actions authorized by a
successful dry run. Bundle provenance, installer and platform upgrades remain
under #261–#263.
