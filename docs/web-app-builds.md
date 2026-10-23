# Build the first web app through the interface

The opt-in Linux runner connects project source, immutable review, explicit owner
approval, real offline compilation and saved results. It builds trusted standalone WASIp2 HTTP libraries using the narrow manifest
contract below. The original [`examples/web-app`](../examples/web-app/) fixture
from [ADR 0004](decisions/0004-first-wasm-web-app.md) remains supported;
[`examples/static-site`](../examples/static-site/) demonstrates another app identity
with packaged assets. It does **not** run or deploy the
component. [The deployment runtime](web-app-deployments.md) validates imports and runs saved releases.
Cloud infrastructure is unnecessary for this step: use a disposable Ubuntu 24.04
x86_64 VM. The later [public cloud acceptance](public-cloud-acceptance.md) records
fresh source installation, DNS/ACME and the connected lifecycle for this profile.

## Application identity and assets

Each reviewed app is a standalone package with a committed Cargo v4 lockfile,
`build = false`, edition 2021 or 2024, conventional `src/lib.rs`, and exactly one
`cdylib` target. `[lib].name` must match the package name with hyphens replaced by
underscores. Package and library names are bounded to 128 ASCII characters; names
start with a letter or underscore, with digits allowed afterward and hyphens only
in the package. Dependency requirements remain exact pins with a complete verified
offline bundle. Workspaces, extra targets, custom library paths, custom Cargo
configuration and unsupported manifest fields remain rejected.

The source snapshot supplies the package/library identity to both review and
execution. The command remains fixed apart from that validated package:

```text
build --release --locked --offline --target wasm32-wasip2 --package sample-site --lib
```

The selected output is `wasm32-wasip2/release/<library>.wasm`. The installed worker
independently derives the same conventional target from its read-only source
manifest; browser input cannot select commands, target paths or compiler flags.
Updating the worker changes the installed configuration fingerprint and requires
fresh review. The immutable release artifact remains `component.wasm` with its
source/review/toolchain/dependency provenance; the runtime does not depend on the
original Cargo output filename.

Package assets as committed regular files and embed them with `include_str!` or
`include_bytes!`. The [static-site example](../examples/static-site/) uses explicit
HTTP routes and correct content types for HTML, CSS and JavaScript. All committed
asset paths and bytes are captured in the source snapshot; edits or an app rename
invalidate prior reviews/approvals. Rebuild to update embedded assets. No runtime
filesystem access, arbitrary URL-to-file mapping, Node pipeline or asset fetching
is added.

Existing bounds still apply: at most 128 source files, 512 KiB per file, 4 MiB
for the complete source snapshot including Git metadata, 4 MiB per component and
1 MiB per HTTP response. Larger sites need measured, separately reviewed changes;
this contract does not silently expand those limits. Runtime HTTP compatibility
validation is still required before deployment.

## Prepare the node

Use the existing unprivileged platform account and existing owner. Do not run the
platform as root. Ubuntu needs systemd, cgroup v2, sudo, Python 3.11+ and the pinned
Rust/Cargo 1.99.0 toolchain including the `wasm32-wasip2` standard library.
Dependency acquisition is an operator setup action, separate from approving a
build. No build downloads packages or accepts shell commands.

The six configured roots must be canonical, pairwise disjoint existing paths with
ASCII letters/digits, `/`, `_`, `-` and `.` only. No symlink components or spaces.
Use private platform-owned directories for owner data, repositories, history and
jobs. For example, the public installation's `/var/lib/devoption/data` and
`/var/lib/devoption/repositories` can coexist with `/var/lib/devoption/history`
and `/var/lib/devoption/jobs`. Keep the real toolchain and vendor tree outside
these four directories, such as under `/opt/devoption/inputs`.

1. Copy the public `examples/web-app` directory to a new trusted standalone Git
   checkout directly under the selected repository root. Commit its three files
   (`Cargo.toml`, `Cargo.lock`, `src/lib.rs`) with your normal Git identity.
   Git metadata must not be group/other writable. The app package and library
   names may instead follow the standalone library contract below. Do not rename
   a real application to the fixture names.
2. Prepare the pinned toolchain and target using your normal verified Rust
   installation procedure. Select its real toolchain directory, not rustup shims.
3. As the platform account, use the pinned Cargo to prepare the fixture's locked
   vendor tree in a separate directory. For example:

   ```sh
   cargo +1.99.0 vendor --locked --manifest-path /var/lib/devoption/repositories/app/Cargo.toml /opt/devoption/inputs/vendor
   ```

   This setup command can use the network. Retain the corresponding `.crate`
   archives from Cargo's registry cache in a separate archive directory. The
   verification utility below hashes each archive against Cargo.lock, checks
   every vendored file against that archive, rejects extra/missing/linked files,
   and seals the complete toolchain/vendor trees. A locally rewritten checksum
   manifest cannot authenticate changed dependency source.
4. Generate configuration as the platform account (substitute real paths):

   ```sh
   python3 scripts/prepare_build_config.py \
     --devoption /opt/devoption/devoption \
     --toolchain /opt/devoption/inputs/rust-1.99.0 \
     --vendor /opt/devoption/inputs/vendor \
     --archives /opt/devoption/inputs/archives \
     --lockfile /var/lib/devoption/repositories/app/Cargo.lock \
     --work /var/lib/devoption/jobs \
     --owner-data /var/lib/devoption/data \
     --repositories /var/lib/devoption/repositories \
     --history /var/lib/devoption/history > build.toml
   ```

   The utility creates no accounts, services or credentials. Input changes need
   a newly verified configuration and service restart, followed by a new review.
5. Have the node administrator install the reviewed helpers and configuration:

   ```sh
   sudo install -d -m 755 /etc/devoption /usr/local/libexec
   sudo install -o root -g root -m 644 build.toml /etc/devoption/build.toml
   sudo install -o root -g root -m 755 scripts/build_broker.py /usr/local/libexec/devoption-build-broker
   sudo install -o root -g root -m 755 scripts/build_worker.py /usr/local/libexec/devoption-build-worker
   ```

   Their ancestor directories must also be root-owned and not group/other
   writable. The broker only accepts `stop` or `run` plus one 32-character hex
   review token. It uses a fixed unit, worker, command and root-owned paths; the
   caller cannot supply a unit name, executable, mount or command line.
6. The administrator must authorize only that broker for the platform account.
   For an account named `devoption`, a `visudo`-checked rule is:

   ```sudoers
   devoption ALL=(root) NOPASSWD: /usr/local/libexec/devoption-build-broker stop, /usr/local/libexec/devoption-build-broker run *
   ```

   The wildcard is validated by the broker's exact argument count/hex grammar.
   Do not grant unrestricted sudo or sudo access to systemd-run. The broker is
   privileged setup; the compiler worker runs as the configured nonroot UID.
   An enclosing platform service must permit this fixed privilege transition:
   the public service example's `NoNewPrivileges=true` must be overridden with
   `NoNewPrivileges=false` for an explicitly enabled build installation. Keep
   `NoNewPrivileges=yes` and empty capabilities on the compiler worker. This
   exception is not needed for installations with builds disabled.

Start the platform with its usual options plus the selected history and explicit
opt-in (the paths must exactly match the installed configuration):

```sh
/opt/devoption/devoption serve --port 3000 \
  --data-dir /var/lib/devoption/data \
  --repository-root /var/lib/devoption/repositories \
  --build-history-dir /var/lib/devoption/history \
  --build-execution enabled
```

For a service, use the [configuration-driven service generator](first-web-app.md#2-supply-the-app-and-install-builddeployment-support)
to include the selected storage and broker privilege requirements. `--public-origin` remains optional and independent.
Missing isolation, unsafe configuration, another runner holding the jobs lock,
or failure to reconcile the previous fixed unit prevents startup. There is no
fallback that runs Cargo directly on the host.

## Use the interface

Create a project, open **Code → Attach repository**, and select the trusted app.
Open **Builds → app · Build app → Prepare build review**. Preparation captures
committed source and verifies inputs; it does not run Cargo. Review the exact
commit, command, target, policy, hashes and complete environment (expand **Input
identities and environment**). Select **Approve and build** once.

Approval expires after five minutes and cannot survive service restart or be
replayed. Changed source, staging, toolchain, dependency bundle or installed
helpers/configuration requires new preparation. Only one attempt runs at a time.
Open the saved attempt and use **Reload saved attempt** for new log checkpoints.
To cancel, return to **Build app → Cancel build**; cancellation is complete only
when the saved state says Cancelled. Cancellation also applies while an approved
attempt is revalidating inputs, before the compiler starts.

Success requires Cargo exit zero, a bounded component header, an independent
artifact copy, complete unit stop and a durable result checkpoint. The output is
`jobs/<review-token>/artifact/component.wasm`, accompanied by `receipt.toml`
containing the job/project/repository IDs, exact commit, source/review/artifact
hashes and artifact length. Match the receipt's job ID to the saved attempt and
recompute the artifact SHA-256 before any downstream handoff. A receipt without
a matching Succeeded history record is not a successful build. The UI shows the
recorded artifact digest and size in **Source and attempt details**; it does not
serve downloads or independently reverify saved artifacts on each page read.
The component header is not proof of the expected HTTP world/imports; the deployment
runtime validates those before instantiation. Historical display fixtures remain records,
not executable artifacts.

## Enforced policy and recovery

`WasmBuildV1` uses a system-level transient systemd unit with a nonroot worker:
one CPU quota, 512 MiB RAM, zero swap, 64 tasks, a 60-second unit lifetime, a
55-second compiler deadline leaving handoff time, and a five-second stop grace.
The supervisor also stops on cancellation or the combined 1 MiB log limit. It
verifies the complete cgroup stopped before recording success/cancellation or
admitting another job. Cleanup uncertainty blocks further attempts until restart
reconciles the unit. Restart never resumes or silently approves queued/running
records; they become Interrupted. Saved output is escaped and bounded.

The worker receives no inherited platform environment, socket access, capabilities
or writable source/dependency/toolchain mount. Home directories, platform data,
repositories, history and host job storage are hidden. Only read-only staged
source and prepared inputs are mapped into `/source`, `/vendor`, `/toolchain`.
The 256 MiB per-job storage envelope reserves 16 MiB for bounded staged source,
selected artifact, logs and metadata; scratch is split into 112 MiB `/job`
(noexec) and 128 MiB `/job/target` (executable). The target exception is required
by the locked dependency's build script. The worker checks effective limits,
read-only mounts, scratch execution flags and denied sockets before compiling.
These restrictions do not make untrusted code supported: selected source,
dependencies, toolchain, OS and other processes under the platform UID remain
trusted. Concurrent out-of-band writes by that UID are outside this contract.

Completed attempts retain source, artifact and receipts on disk; there is no
retention/garbage-collection UI yet. Stop the service before operator maintenance;
never remove a running attempt or rewrite a recorded successful artifact.

## Local evidence and remaining limits

Validation uses the existing disposable Ubuntu 24.04 x86_64 VM, not a production
server. The real interface successfully built revision A (123,606-byte component,
SHA-256 `d085e733cfdb994e7541c3333a8f12220098bef984d7707310b2425c1e8b33f1`)
and saved its compiler output and provenance. Local interface checks also verified:

| Case | Observed result |
| --- | --- |
| Source edited after review | Approval rejected; no build unit started |
| Explicit Rust compilation error | Failed with the compiler diagnostic retained |
| Cancel while the compiler unit is active | Cancelled; unit inactive and no remaining cgroup |
| Allowed compile-time loop | Failed as timed out; worker exit 124 after about 55 seconds, diagnostic retained |
| Kill the temporary platform while its build unit is active | Restart stopped the unit; history became Interrupted; no automatic resume |
| Another build after terminal results | A fresh review was admitted without restarting the service |

These are disposable trusted fixtures, not adversarial workload certification.
The node's existing baseline owner/service was kept separate from the temporary
build installation. The PR records final checks and cleanup.
The supported target is intentionally narrow. Runtime requests, deployment,
public app DNS/HTTPS and fresh cloud installation are not yet verified here.

Successful builds can next be [saved and deployed through Releases](web-app-deployments.md) when the Linux app runtime is explicitly installed and enabled. New receipts retain explicit compiler, dependency and configuration digests for immutable release provenance.
