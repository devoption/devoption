# Check source before a build

Open a project’s **Builds** page, then choose **Check source** for an attached
repository. The report shows its observed HEAD, working-tree changes, fixed source
file checks, and package/dependency metadata when Cargo input inspection succeeds.
**Check again** reads the repository again; **Open Code** returns to its files and
attachment. Links work with keyboard navigation, refresh, history and without
JavaScript after sign-in.

- **Source checks passed** means the initial checks below passed. It does not mean
  a build succeeded, source was compiled, or execution is approved.
- **Changes needed** identifies missing commits/files, Git changes or unsupported
  Cargo inputs. Resolve them using Git or your editor outside DevOption and check
  again. DevOption does not edit, stage, commit or discard source.
- **Source checks unavailable** means storage, a changed attachment, file safety,
  encoding or inspection limits prevented a check. It never substitutes success
  for a failed read.

Reports are fresh observations of the current working tree, not verified immutable
source snapshots. No job, durable preparation record, build history or approval is
created. Build execution remains unavailable. The flow runs no Cargo, compiler,
project code, dependency acquisition, network fetch or VM.

## Current checks

The association must have a saved default checkout under the configured inspection
or managed root. A passing report requires an observed HEAD commit, no Git-reported
staged/modified/untracked/conflicted entries, and readable, nonempty tracked text in
these three fixed paths: `Cargo.toml`, `Cargo.lock`, and `src/main.rs`.

The [existing Cargo adapter](cargo-inputs.md) checks the manifest and lockfile for
internal consistency: one standalone package, required name/version/edition and
`build = false`, exact direct version pins, a version-4 lockfile with a matching
root package, and the supported closed crates.io dependency graph. Workspaces,
path/Git dependencies and unsupported manifest keys fail this check. The report
shows the actual inspected package name/version and locked registry dependency
count. It does not select a registry or manufacture materialization metadata.

`src/main.rs` is checked for tracked text presence only. Its Rust syntax, target
semantics and buildability are not checked. Tests deliberately use invalid Rust
text to establish that this check does not invoke a compiler.

## Bounds and trust

The existing [file browsing bounds](repository-files.md) apply: up to 256 tracked
paths, paths of at most 1024 UTF-8 bytes/16 components, an index of at most 1 MiB,
and each selected text file at most **64 KiB**. This runtime lockfile bound is
narrower than the pure Cargo adapter’s 512 KiB limit. Only the three fixed tracked files supply text to the Cargo input/source checks.
Status retains the existing Git ignore handling. Symlinks, hard links, unsupported entries, binary/unsupported-control text
and unsafe paths cannot provide source inputs.

The project domain reads status before and after the three file reads and checks
Git metadata/index plus root/checkout identity across the operation. It runs two
controlled `git status` commands and one `git ls-files`, each with a three-second
deadline and 64 KiB output cap, using private temporary metadata and no source
index writes. The existing status path can read source objects; the tracked-path
list uses an empty private object directory. Detected changes reject the report.
External filesystem writers remain trusted; repeated observations are not an atomic
snapshot, and Git status is not proof that all bytes match a commit.

`GET`/`HEAD /projects/{project}/builds/{repository}` require the existing session
when an owner is configured. All responses use `Cache-Control: no-store`. A missing
project or foreign repository association returns 404; changed bindings return
409; failed repository inspection returns 503. A completed report with missing or
unsupported source inputs returns 200 with its actual result. No credentials or
source contents enter URLs, logs or the report HTML.

## Remaining build work

This flow is separate from the pure [BuildPlan](build-plans.md) and
[job lifecycle](build-jobs.md) models. A runnable workflow still needs a verified
immutable source/target, prepared and verified dependency archives, selected and
verified toolchain/executable, owned work directory, execution policy and an
explicit approval bound to those facts. Job persistence, bounded execution/logs,
cancellation and restart recovery are also not implemented. No passing report can
be submitted as an execution capability.

Implemented and tested with disposable repositories under
[#171](https://github.com/devoption/devoption/issues/171).
