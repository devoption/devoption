# Release preparation dry run

The preparation work under [#260](https://github.com/devoption/devoption/issues/260)
produces a reviewable preview version/changelog proposal and can optionally
[create a bot-owned draft release PR](release-prs.md). Default proposal mode does not merge, tag,
create GitHub releases, distribute bundles or upload to a registry. The existing
[release approval process](releases.md) and #21 remain applicable. There is still
no published product release; the prepared source version is `0.1.0-preview.2`.
Its schema-2 metadata declares only the verified preview.1 base-bundle upgrade
path. Signed-packet acceptance and publication approval remain pending; see
[platform upgrade limits](platform-upgrades.md).

## Run a proposal

Dispatch **Release preparation** from `main`, supply an explicit proposal date,
and leave `create_draft` unchecked for the read-only run. It runs the existing
full CI workflow first. Only successful checks allow
the proposal job to run, against that run's exact checked-out commit, even if
`main` advances later. The job installs a checksum-pinned release-plz 0.3.172,
fetches locked Cargo inputs and runs the tool's disposable fixtures. Its summary
contains a JSON report and proposed patch. The checks/proposal jobs have read-only
permissions, checkout credentials are not persisted, and no forge token is
supplied to release-plz. Selecting `create_draft` authorizes a separate writer job
with narrowly scoped write permissions; see its authentication and retry limits
in [draft release PR generation](release-prs.md). Runs share a concurrency group;
a newer dispatch does not cancel an in-flight run.

For a local run, install that exact tool version from the
[official release](https://github.com/release-plz/release-plz/releases/tag/release-plz-v0.3.172)
and verify the archive before executing it. The workflow pins the Ubuntu x86_64
GNU/Linux archive to SHA-256
`84b7013d164e1fc780b31dc9ef190977830599d6cb5ce81c993f6fcbd3cdc1aa`.
The macOS ARM64 archive used for local verification has SHA-256
`1b2019acd2f218730d85d9426387c872ee830e73261218b6863b6e814dde86b0`.
Both match the official GitHub release asset digests checked on 2026-10-18.
This pins downloaded bytes; it is not build-provenance verification for DevOption
bundles (tracked separately in #261).

The GNU/Linux build replaces the musl build after the first opted-in
[main run](https://github.com/devoption/devoption/actions/runs/39155120245)
failed in four analysis fixtures before the writer ran. A diagnostic PR run
reproduced `stack limit reached` while parsing the changelog template. Ordinary
read-only PR CI now runs the real-tool fixtures as well, rather than skipping
them until manual preparation.

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
[release metadata record](release-metadata.md). Current-version metadata is bound
to the selected preview automatically in the proposal; only its version changes.
Its support, compatibility,
migration and recovery statements are appended as a checked block; the report
records the exact metadata digest. Missing, unrelated stale or inconsistent metadata blocks
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

Normal Python CI and the manual proposal workflow both install the pinned tool
and run these fixtures. Local runs without `DEVOPTION_RELEASE_PLZ` skip the
tool-dependent cases. Fixtures use disposable Git tags and
small local crates, and test version groups, no-release behavior, breaking
changes, preview numbering, helper-only changes, baselines, deterministic retries
and failures.

## Implementation and activation status

- The [live draft writer](https://github.com/devoption/devoption/actions/runs/39156408201)
  generated merged PR #270, and its exact-main Release check passed.
- [Guarded optional auto-merge](release-prs.md#optional-guarded-auto-merge) is
  implemented and covered by fixtures, but checked-in and repository policy remain
  disabled. Enabling those policies or approving bot workflows is an owner decision.
- Preparation, signing and [reserved-candidate retention](release-reconciliation.md)
  share serialization. Retention retries reconcile exact immutable identities;
  public publication remains separately gated by #21 and #261/#263.

The [release status table](releases.md#delivery-status-against-the-six-issues)
maps the original issues to actual delivered behavior and remaining work. Manual
dispatch remains the default activation policy; version generation itself needs
no manual Cargo version edits. Installer and platform upgrades remain #262/#263.
