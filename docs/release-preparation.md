# Release preparation dry run

This first slice of [#260](https://github.com/devoption/devoption/issues/260)
adds a reviewable version/changelog proposal. It does not create a release PR,
merge, tag, GitHub release, bundle or registry upload. The existing
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
  --date 2026-10-18 \
  --output /tmp/devoption-release-proposal
```

Use the intended proposal date; omitting `--date` uses the local current date.
Choose a new output directory outside the source checkout. The script clones the
exact source commit to temporary storage, runs `release-plz update` offline there,
checks the resulting workspace/lockfile versions and allowed file changes, and
writes `report.json` and `proposal.patch`. The original checkout, commits and tags
stay unchanged. A failed tool/check produces no successful output directory;
retry with the same source, tags, tool and date produces the same proposal. This
is not a test of partial *publication* recovery: no publication path exists here.

## Version and notes behavior

The [release-plz configuration](https://release-plz.dev/docs/config) uses Git tags
matching `v{{ version }}`, rather than crates.io, as version baselines. All seven
workspace crates inherit one workspace version, remain `publish = false`, and
belong to the same version group. Only the CLI package writes the root changelog;
its generated notes include changes from all the other crates. Existing human
changelog entries are retained. Generated commit summaries still require review
for accuracy, omissions, duplicate descriptions and issue links.

For stable-version fixtures, `fix` and `perf` changes select a patch, `feat` selects
a minor even in 0.x, and a breaking `!` or `BREAKING CHANGE:` footer selects a minor
in 0.x (a major after 1.0). Documentation-only changes do not trigger a release.
Conventional PR titles continue to be checked by the existing contribution job.
No library API compatibility guarantee is inferred: semver API checking is
disabled for these internal, unpublished crates.

**Preview selection is not implemented.** Actual pinned-tool fixtures show that
an initial `0.1.0-dev` stays unchanged, and a tagged `0.3.0-preview.1` stays unchanged
after a fix. The report flags this limitation; a nonempty changelog diff is not
evidence of a new publishable version. The intended first distribution channel is
preview, with an explicitly selected embedded version before building. Automatic
preview numbering and stable promotion must be implemented and tested before
publication automation is enabled.

Run all fixtures with the real pinned binary:

```sh
DEVOPTION_RELEASE_PLZ=/absolute/path/to/release-plz \
  python3 -m unittest discover -s scripts -p test_release_plan.py
```

Normal Python CI runs the boundary checks and skips the tool-dependent fixtures;
the manual proposal workflow requires them. Fixtures use disposable Git tags and
small local crates, and test version groups, no-release behavior, breaking
changes, prerelease limits, deterministic retries and failures.

## Remaining #260 work before enablement

- Cover shipped files outside Cargo packages (for example Python helpers and
  service templates). Crate-based release-plz analysis alone can miss their
  changes; the report always records this limitation.
- Select and implement automatic preview numbering and checked-in migration,
  compatibility and support metadata. Missing required metadata must block an
  actual candidate; this dry run never marks one eligible.
- Generate narrowly scoped bot-owned draft release PRs with issue-linked commits,
  required checks on their exact revision and a reviewed authentication approach.
  No auto-merge or repository-policy change is included here.
- Reconcile #21 with ongoing publication authority, serialize publication with
  preparation, and test duplicate-version, partial-publication and retry recovery
  against immutable artifact identities. Version proposals alone cannot do this.

These are remaining implementation/approval gates, not actions authorized by a
successful dry run. Bundle provenance, installer and platform upgrades remain
under #261–#263.
