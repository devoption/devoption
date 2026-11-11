# Build and download a reusable library

[#384](https://github.com/devoption/devoption/issues/384) adds a separate
`devoption_builds::execution::prepare_library` API for trusted local adapters.
It uses existing registered source snapshots, verified operator-prepared
inputs, immutable review, single-use approval, persisted job state/output,
cancellation and complete-unit cleanup. Signed-in workspace users can prepare and approve
library builds through project Builds, then download a retained source archive
and receipt from the successful attempt. [Package distribution](reusable-packages.md)
uses a separate explicit staging/publication/project-grant workflow. A library
build alone does not publish a crate or authorize a consumer.

## Use the project interface

Register the separate library repository and use the existing
[build-input setup](web-app-builds.md#prepare-the-node) with its committed lockfile,
verified vendor/archive set and pinned toolchain. Open **Builds → Check source**,
then **Continue to build review** (or the repository's **Build** link).
Opening the page runs no build. **Prepare build review** captures committed source
and derives the profile from the verified manifest. A caller cannot choose a
profile, shell command or target through the request. The ordinary app-only
runner API still refuses `rlib`; the authenticated dispatcher calls the dedicated
library API for a captured `rlib` source.

The prepared library review identifies native compilation/unit tests, WASIp2
compilation, `source.tar`, resource/network limits, and all three exact commands
inside **Build command, inputs & technical details**. **Approve and build** is a
separate single-use authenticated action, expiring after five minutes. Source,
catalog, configuration or prepared-input changes require a new review. Cancellation and
saved history use the existing build lifecycle; an ambiguous submission must be
checked in history rather than automatically retried.

After success, open the saved attempt and select **Download source package** or
**Download build receipt**. These are authenticated workspace downloads, not
registry endpoints. Both revalidate the successful project/repository/attempt,
the recorded work-root/job identity, private regular-file bounds, receipt
provenance, and actual archive SHA-256/size. Symlinks, hard links, substituted
roots/receipts, corrupt bytes and unsafe permissions are refused. Reads serialize
with retention and return independent bounded bytes; pruned, unsuccessful and
non-library attempts have no package. Verification failures are shown separately
from saved build status. Downloads need the configured build root and may become
unavailable after retention cleanup. Nothing is fetched from a mutable checkout.

The routes are `GET /projects/{project}/builds/attempts/{attempt}/package` and
`GET /projects/{project}/builds/attempts/{attempt}/package/receipt`, behind the
existing workspace-session and Host guards. They use fixed attachment filenames,
explicit content types, `nosniff` and `Cache-Control: no-store`. Access follows
the existing [shared-project membership policy](users.md); this adds no
project-specific user roles. All mutation routes retain Origin/CSRF/session checks. Source checks and HTML review alone do
not approve execution.

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
change. The deployment broker refuses these receipts as app releases. Downloading a
package does not protect it from the project's configured build retention policy.

This archive is a local verified source package, not a published Cargo `.crate`.
Registry ingestion must validate it, establish namespace/visibility/version policy
and prepare the registry archive/index under [#385](https://github.com/devoption/devoption/issues/385).
A package version or digest does not itself authorize publication.
The separate [source-staging API](library-source-staging.md) can privately retain
canonical archives from verified source plans. It does not import retained build
receipts or attest build success, and is not connected to this download flow.
The [Cargo generator](cargo-package-generation.md) produces candidate `.crate` and
index bytes from library plans; publication/registry ingestion remain separate.

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
source preservation/archive bytes and receipt, native generation/staging/reopen of
a Cargo candidate from the successful retained build with its exact receipt,
refusal to overwrite that version from a second successful revision, stale review
refusal, failing unit tests, running cancellation and queued-job interruption
recovery. Python independently reads the archive and compares every file against
the selected fixture, then builds/runs a separate native consumer and compiles it
for WASIp2. That consumer uses a temporary Cargo path dependency solely as fixture
wiring; DevOption still rejects path dependencies. This does not claim registry
admission, global availability or production rollout. The same disposable
checker also starts the actual controller and exercises owner login, source/review
pages, explicit approval, invalid-token/replay and missing-CSRF refusal, saved
archive/receipt download, cross-project refusal, corrupt archive refusal and
package access after controller restart. Renderer captures are explicitly
synthetic UI evidence; the Linux checker supplies execution/HTTP evidence.

With an explicitly configured private registry root, the controller can also
[review and stage a candidate](registry-controller.md#successful-build-candidate-staging)
from a successful retained attempt at its exact clean registered revision. Staging
preserves the exact receipt and source provenance without executing another job.
Publication/project reader grants require their own review and approval.

The same guarded installed acceptance also builds the public static-site example
with a registry-enabled controller, first with public dependencies and then with
the published dependency-free library used by its HTTP handler. Both components
undergo runtime validation and health/routing/exact-asset HTTP checks. The fixture
checks archive mutation after review, stale source reviews and revoked mixed-graph
grants. It uses the committed public archive fixtures; the worker stays offline.
See the [supported closure policy](cargo-vendor-bundles.md#pinned-public-wasip2-closure).
For a before-fix reproduction using an unpatched controller binary, set
`DEVOPTION_EXPECT_WASIP2_REFUSAL=1` on the disposable acceptance command: the
public static-site preparation must return 409, while library-only consumers
still build. This mode is a reproduction, not a successful WASIp2 acceptance.
