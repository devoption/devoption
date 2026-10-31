# Unified app-capable source installation

On a **fresh dedicated Ubuntu 24.04 amd64 host**, one reviewed configuration
drives installation of the platform, Wasm runtime, build/deployment helpers,
private storage, narrow sudo rules and systemd services. Python remains a runtime
dependency; operators no longer install those helpers separately. This is a
prepared source-build path, not a signed bundle installer, upgrade, conversion or
published release channel. Existing installations must retain their maintenance
procedures.

## Prepare inputs

Keep the [public installation prerequisites](public-linux-install.md#supported-shape-and-prerequisites):
systemd/cgroup v2, administrator access, Python 3.11+, Git, sudo including `visudo`,
Ubuntu `build-essential` (including `cc` and native development libraries), and the
standard account/systemd tools. Cargo's native build-time dependencies need the
C linker even when the final app targets Wasm; plan refuses a missing `cc` before
installation changes. Ports 80, 443, 3000 and private app port
15000 must be free. Arrange platform DNS and firewall access separately. At least
1 GiB plus twice the installed payload size must be free on `/opt`, `/var/lib`
and `/etc`; prepared toolchain inputs and app builds need additional space.

Use an independently reviewed checkout and these explicitly prepared inputs:

- Linux `devoption` and `devoption-runtime` from the reviewed source, built with
  the pinned toolchain using `cargo build --workspace --release --locked --offline`.
  Independently record their SHA-256 values from that reviewed build.
- The [pinned Caddy archive](public-linux-install.md#install-the-pinned-proxy-and-configure-tls).
  The installer checks its trusted SHA-512 pin.
- A real Rust/Cargo 1.99.0 toolchain with `wasm32-wasip2`, a Cargo vendor tree,
  the corresponding standalone app lockfile and original crates.io `.crate`
  archives, prepared using the [build input procedure](web-app-builds.md#prepare-the-node).
  Vendored files are authenticated against the original lockfile archive hashes.

Use canonical absolute ASCII paths without spaces or links. Toolchain, vendor
and archive roots must be separate and outside installation-owned storage, for
example `/opt/devoption-inputs/{rust-1.99.0,vendor,archives}`. Toolchain/vendor
trees must be administrator-owned, not group/other writable, and readable and
traversable by the service account. They remain at these locations; the installer
does not copy, chmod or delete them. Keep the original binaries/lockfile outside
installation-owned paths too.

## Generate the installation configuration

Keep prepared inputs together in an administrator-owned directory such as
`/srv/devoption-stage`, with this layout:

```text
devoption-stage/
  devoption
  devoption-runtime
  caddy.tar.gz
  toolchain/       # Rust/Cargo 1.99.0, including wasm32-wasip2
  vendor/
  Cargo.lock
  archives/       # Original checksum-matching crates.io archives
```

The command below replaces manual hash calculation and TOML transcription. It
checks the canonical paths, ownership/modes, supported toolchain files, vendor
archives and pinned proxy, then records the executable, lockfile and tree hashes
in the existing schema. It reads the selected files without executing binaries,
Cargo or project code, downloading inputs, inspecting installed accounts or
changing services:

```sh
sudo python3 scripts/install_app.py configure \
  --inputs /srv/devoption-stage \
  --hostname platform.example.com \
  --source-commit FULL_REVIEWED_40_CHARACTER_SOURCE_COMMIT \
  --output /srv/devoption-stage/installation.toml
```

Use the full lowercase source commit explicitly recorded during your reviewed
build. Review the generated paths, hostname, source identity and hashes before
planning. Hashing selected bytes records their identity; it does not independently
prove where those bytes came from or that the executable matches the declared
source commit. There is no signed-bundle or acquisition-authenticity claim.
Runtime version checks still run during installation validation.

The output parent must already exist, be canonical and administrator-controlled.
The new file is private (`0600`) and published atomically without replacement.
Existing files, directories and symlinks are refused, including partial files from
earlier attempts. Choose a new output in the staging directory; system
configuration locations, installed storage and hashed input trees are refused.
If publication or its final sync fails, inspect any existing output before another
attempt. A published file is retained; interrupted temporary files are never
adopted or automatically removed by later runs. No installed state is reset.

The prepared inputs stay at these paths. Keep the generated configuration and
original inputs for matching retries. Input trees retain the existing bounds of
16,384 regular files and 2 GiB per tree; avoid unnecessary toolchain components.
Dependency acquisition remains a separate operator preparation step.

### Manual configuration remains supported

Existing manually reviewed schema-1 configurations remain supported. If inputs
are kept in separate locations, record their identities using the trusted
`build-input-digest` command and use the existing format:

```toml
schema_version = 1
hostname = "platform.example.com"
source_commit = "FULL_REVIEWED_40_CHARACTER_SOURCE_COMMIT"
binary = "/srv/devoption-stage/devoption"
binary_sha256 = "REVIEWED_64_CHARACTER_SHA256"
runtime = "/srv/devoption-stage/devoption-runtime"
runtime_sha256 = "REVIEWED_64_CHARACTER_SHA256"
caddy_archive = "/srv/devoption-stage/caddy.tar.gz"
toolchain = "/opt/devoption-inputs/rust-1.99.0"
toolchain_sha256 = "REVIEWED_BUILD_INPUT_DIGEST"
vendor = "/opt/devoption-inputs/vendor"
vendor_sha256 = "REVIEWED_BUILD_INPUT_DIGEST"
lockfile = "/srv/devoption-stage/Cargo.lock"
lock_sha256 = "REVIEWED_64_CHARACTER_SHA256"
archives = "/opt/devoption-inputs/archives"
```

The source commit is the operator's recorded binary source identity; the hashes
bind selected bytes to that review. This is not a Sigstore attestation or proof
that arbitrary binaries were compiled from the named commit. Both executables
must report the trusted checkout's platform version. Retain the configuration,
original inputs and installer checkout for matching retries.

## Plan, apply, enroll and start

From that trusted checkout:

```sh
sudo python3 scripts/install_app.py plan --config /srv/devoption-stage/installation.toml
sudo python3 scripts/install_app.py apply --config /srv/devoption-stage/installation.toml
```

`plan` reports accounts to create, fixed storage roots, missing files and the next
action. It checks prerequisites, identities, paths, existing accounts/services and
ports without writing installation resources. `apply` repeats checks under the
shared installer lock, creates dedicated non-login accounts and generates matching
configuration using the actual assigned platform UID. It installs these resources:

| Resource | Location |
| --- | --- |
| Source-built executables | `/opt/devoption/source/SOURCE_COMMIT/bin/` |
| Pinned proxy | `/opt/devoption/proxy/` |
| Owner/catalog/profile, repositories, history, jobs | `/var/lib/devoption/{data,repositories,history,jobs}` |
| Protected app releases and intent | `/var/lib/devoption-app` |
| Build/deployment configuration | `/etc/devoption/{build,deployment}.toml` |
| Fixed build/deployment helpers | `/usr/local/libexec/devoption-*` |
| Narrow broker authorization | `/etc/sudoers.d/devoption-app` |
| Platform/proxy units | `devoption-public.service`, `caddy-devoption.service` |
| Private installation receipt | `/var/lib/devoption-installer/installation.json` |

The platform service enables managed repository creation, history, builds and
deployment. Its existing narrow broker privilege transition requires
`NoNewPrivileges=false`; compiler/app worker isolation stays unchanged. Apply
validates services, proxy configuration, sudo rules, executable versions and
input readability, then reloads systemd. It does **not** start services, enroll an
owner, create projects or execute builds/deployments.

In an interactive local/SSH terminal, use the existing hidden password prompts:

```sh
sudo python3 scripts/install_app.py enroll --config /srv/devoption-stage/installation.toml
sudo python3 scripts/install_app.py start --config /srv/devoption-stage/installation.toml
sudo python3 scripts/install_app.py status --config /srv/devoption-stage/installation.toml
```

Enrollment never replaces an owner. Start requires a complete matching installation
and owner, validates configuration, enables/starts services and checks the private
sign-in redirect. It does not prove public DNS/ACME. Verify HTTPS sign-in externally.

Create a project, open **Code → New repository**, and choose the starter web app
to create committed source without SSH. You can also attach existing committed
source. Then use the existing [browser build/release workflow](first-web-app.md#3-build-and-deploy-from-the-interface).
The initial runtime has one private app listener on port 15000. No project IDs or
app hostnames are invented. Configure the [app hostname](app-hostnames.md) after
project creation, or follow [multi-app configuration](multiple-apps.md). Those
changes need their own review and fresh deployment approval. Arbitrary dependency
acquisition, source authoring, database grants and secret keys remain separate.

## Preservation and retry rules

- Unmanaged source/bundle installations, conflicting accounts/services/storage
  and foreign receipts are refused before installation changes. Matching file
  bytes alone do not authorize adoption. There is no force, overwrite, delete,
  account reset, automatic repair, upgrade or conversion option.
- Identical retries create only missing owned files, using atomic commits without
  replacement. Different files/modes, links, hashes, hostname or receipt stop the
  command. Owner/catalog/profile, repositories, history/jobs, releases/intent,
  proxy state and unrelated shared-directory files are preserved. Reapplying does
  not restart services or apps, and repeat enrollment retains the owner.
- Operator edits, including later app-hostname configuration, are preserved and
  cause a retry to refuse instead of reverting them. Use the relevant maintenance
  procedure for changed configurations. The source app layout remains outside
  the [bundle upgrade](platform-upgrades.md) support envelope.
- Committed-file interruptions resume with identical inputs. An unreceipted
  partial directory, incomplete account creation or unsafe partial ownership needs
  operator inspection; the installer does not guess ownership or delete evidence.
  Failed apply may leave its own receipt/files for inspection. Preserve them and
  the original inputs; never remove a receipt to bypass a conflict.

## Validation scope

Disposable filesystem/host tests cover planning/application, generated UID/path
consistency, committed interruptions, unchanged file inodes, owner/project/source/
build/release preservation, foreign receipts, operator edits, links, changed input
hashes, missing target inputs and occupied ports. Linux CI additionally exercises
real systemd/sudo parsers and compares tree identity with the actual Rust reader.
The existing bundle tests continue to cover the shared no-replacement engine.

Those unit tests model account/service actions. A separate fresh-VM acceptance
harness now exercises real configure/installation commands and authenticated
starter, build and deployment actions, including configuration no-replacement
checks:

```sh
python3 scripts/check_app_installation.py --prepare /absolute/new-inputs \
  --binary target/release/devoption --runtime target/release/devoption-runtime \
  --caddy-archive /absolute/pinned-caddy.tar.gz
python3 scripts/check_app_installation.py --inputs /absolute/new-inputs \
  --report /absolute/new-report.json
```

Prepare as an unprivileged Linux user with the pinned toolchain/target and original
crate cache available. It vendors the public static-site fixture and copies a
bounded toolchain and reviewed binaries into a new directory. Host mode requires
QEMU/qemu-img and xorriso (or macOS hdiutil), downloads the existing pinned Ubuntu
image and boots a fresh private disk. An optional `--base-image` accepts only the
same pinned image hash. The guest installs Git/sudo/build-essential from Ubuntu archives; package
acquisition needs network access. No host root, SSH credentials, forwarded ports
or host trust changes are needed. The VM and its copied state are removed afterward.

Acceptance uses the real `plan`, `apply`, `enroll`, `start` and `status` commands,
synthetic terminal enrollment, test-CA HTTPS sign-in, a managed repository, approved
offline build, saved release/deployment, identical retry preservation, operator-edit
and changed-input refusal, and running/stopped app recovery after service restart.
The proxy has a guest-only drop-in for loopback/test-CA TLS; the installed production
Caddyfile stays unchanged. Private owner/application state stays inside the VM.
The report contains only exact source/binary/image identities and completed cases.
Missing cases, failed checks, stale output and mismatched identities cannot pass.
Linux installation CI runs this harness and requires the complete report.

This is single-node local HTTPS acceptance, not public DNS, ACME, a kernel reboot
test, arbitrary-framework support or a release approval. Existing
[first-app public evidence](first-web-app.md#recovery-boundary-and-acceptance-record)
retains its own scope; it is not automatically acceptance of this entry point.
