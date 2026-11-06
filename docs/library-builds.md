# Reusable library build API

[#384](https://github.com/devoption/devoption/issues/384) adds a separate
`devoption_builds::execution::prepare_library` API for trusted local adapters.
It uses existing registered source snapshots, verified operator-prepared
inputs, immutable review, single-use approval, persisted job state/output,
cancellation and complete-unit cleanup. This is a runner implementation;
the browser's **Build app** action remains an app-only flow. There is no new
user-facing library command, HTTP action, package download or registry here.
Those controls must supply an authenticated explicit review/approval action
before calling `execute`; preparing source or constructing a plan grants no
execution authority. [Package distribution](reusable-packages.md) is separate.

## Selected profile

A library uses the [explicit standalone `rlib` source shape](cargo-inputs.md),
Rust/Cargo 1.99.0, a version-4 lockfile and verified offline dependencies.
The conventional library name must match the package name with hyphens changed
to underscores. Inline `#[test]` unit tests in the captured source are supported;
additional `tests/` targets, dev dependencies, doctest execution, features,
workspaces and custom targets remain outside this profile.

The immutable plan exposes all three fixed commands through `library_commands()`:

```text
cargo build --release --locked --offline --package PACKAGE --lib
cargo test --release --locked --offline --package PACKAGE --lib
cargo build --release --locked --offline --package PACKAGE --lib --target wasm32-wasip2
```

Each command must succeed before the next runs. Native unit tests execute in the
worker's existing executable target scratch. WASIp2 compilation checks target
compatibility; it does not execute Wasm unit tests or validate an HTTP component.
Native means the installed Linux toolchain's host target, not a caller-selected
triple. The policy fingerprint is distinct from both `InitialBuildV1` and
`WasmBuildV1`; their review contexts cannot authorize this profile.

All three commands share the existing worker envelope, not three separate budgets:
60 seconds for the unit, 55 seconds for the compiler sequence, 512 MiB memory,
zero swap, one CPU, 64 tasks, 1 MiB combined logs and the existing 256 MiB storage
reservation. No inherited credentials or network/sockets are admitted. Source,
vendor and toolchain mounts remain read-only. See the [build boundary](web-app-builds.md#enforced-policy-and-recovery)
for trust limits, stop/restart handling and prepared input setup. The same config
must match the library's exact lockfile; a different dependency graph requires
operator preparation and new review. Larger libraries may exceed these bounds;
no larger resource policy is claimed.

## Source output and provenance

After successful compilation/tests and confirmed cgroup stop, the supervisor
requires an exact library completion record from the installed worker. It then
creates `artifact/source.tar` from its owned verified source snapshot, never from
compiler scratch or a mutable checkout. Native and Wasm `.rlib` files are not
retained as distributable artifacts. The ordinary app preparation API rejects
`rlib` input before staging or queueing a job.

The source archive is deterministic uncompressed POSIX ustar (`ustar-source-v1`):
all captured regular files in snapshot order, under `PACKAGE-VERSION/`, mode 0644,
UID/GID/timestamp zero, no owner names, links, executable files or synthesized
manifest. Blocks are zero padded, followed by two zero blocks. Each archive path
must fit ustar's 100-byte name and optional 155-byte prefix; unsupported longer
paths and archives exceeding 4 MiB including headers/padding are refused before
review. Existing snapshot file/byte limits still apply. All source is included;
there are no publishing ignore rules, so source selection must be reviewed.

The existing private receipt includes job/project/repository, exact commit,
source/review/toolchain/dependency/configuration hashes and the actual archive
SHA-256/size. Library receipts identify `source.tar`, `LibraryBuildV1`,
`native+wasm32-wasip2`, package name/version and `ustar-source-v1`; they contain no
HTTP ABI claim. Saved success and matching receipt are both required. Existing
history recovery and retention apply to library jobs without a history schema
change. The deployment broker refuses these receipts as app releases.

This archive is a local verified source package, not a published Cargo `.crate`.
Registry ingestion must validate it, establish namespace/visibility/version policy
and prepare the registry archive/index under [#385](https://github.com/devoption/devoption/issues/385).
A package version or digest does not itself authorize publication.

## Validation

Ordinary Rust/Python tests exercise fixed commands, policy separation, invalidated
approval, exact source packaging, ustar checksums/path/size limits, worker shared
budgets and failed-phase stopping, and app promotion refusal.

The explicitly disposable Linux checker uses a separate trusted library repository
and the real installed broker/worker through `prepare_library`/`execute`:

```sh
python3 scripts/check_library_build.py --disposable-system-host --binary target/release/devoption
```

It refuses an existing configuration/helper installation or build unit before
mutation, installs temporary helpers/configuration on the disposable host, and
cleans them up. It needs pinned native/WASIp2 toolchains, systemd/cgroups, a linker
and passwordless fixture sudo, as in existing build acceptance. The ordinary CI
bundle suite runs it; never run it against an existing installation.

The checker covers native compilation/unit tests, WASIp2 compilation, exact
source preservation/archive bytes and receipt, a second revision, stale review
refusal, failing unit tests, running cancellation and queued-job interruption
recovery. Python independently reads the archive and compares every file against
the selected fixture, then builds/runs a separate native consumer and compiles it
for WASIp2. That consumer uses a temporary Cargo path dependency solely as fixture
wiring; DevOption still rejects path dependencies. This does not claim registry
admission, global availability, user-facing library controls or production rollout.
