# Proposed one-shot local runner rehearsal — not approved or implemented

Scope: one trusted `offline-fixture` attempt from this repository's reviewed merge
commit for [PR #128](https://github.com/devoption/devoption/pull/128). Use only the
checked-in fixture source, its exact lockfile,
the pinned `itoa` seed and fixed local-registry context. Record their hashes and
the selected commit before an approval. No registered/user repository, real
instance, server route/UI, production event issuer, release or deployment.

- Host: the existing disposable Ubuntu 24.04 amd64 rehearsal guest, started only
  after separate approval. Reuse its installed Rust 1.99.0 toolchain and system
  linker after checking versions and executable hashes. Abort if any required
  tool or isolation control is missing; no downloads or package installation.
- Identity: one transient systemd service with `DynamicUser=yes`, no credentials,
  no supplementary privileges, no capabilities and `NoNewPrivileges=yes`.
  A separately approved harness may create that unit and its owned directories;
  the build itself never runs as root. No persistent account or host service.
- Files: immutable fixture/toolchain mounts; only `/run/devoption-runner/attempt-1`
  writable, backed by a fresh 256 MiB tmpfs containing separate source staging,
  Cargo home, HOME, scratch and target subdirectories. Mount final source read-only
  before launching. Hide guest home/credential paths, use private devices and
  prohibit access to host shares/sockets. No writable shared Cargo cache.
- Network: a private network namespace with no external interface, plus a denied
  socket-address-family policy for the job. Existing guest management access stays
  outside the job. Verify denied connectivity in the disposable environment before
  invoking Cargo; Cargo offline mode alone is insufficient.
- Environment: clear it, then supply only the exact verified toolchain/system
  PATH, owned HOME/CARGO_HOME/CARGO_TARGET_DIR/TMPDIR, explicit RUSTC, LANG=C.UTF-8,
  CARGO_NET_OFFLINE=true, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=1 and empty Rust
  compiler wrappers. No inherited tokens, flags, config or credentials.
- Command: direct argument vector, no shell: `cargo build --release --locked
  --offline --package offline-fixture --bin offline-fixture`.
- Limits: one active job, one CPU quota, 512 MiB memory, zero swap, 64 processes,
  60 seconds elapsed, 1 MiB combined captured logs, 4 MiB maximum artifact.
  On timeout/cancel, stop the complete service cgroup, allow five seconds, then
  kill and confirm no descendants remain before recording a terminal outcome.
- Result: hash and size-check the actual regular binary from the owned target
  directory after successful exit; reject symlinks, missing or oversized artifacts.
  Optionally executing it is outside this proposed first attempt. Exercise failure
  and cancellation separately only after extending the approved attempt count.
- Retention: no executable export; delete the complete owned job tree after the
  result. Retain only bounded logs and a metadata receipt in the ignored rehearsal
  directory, then stop the guest. Preserve the total 4 GiB host allocation budget,
  stop at 3.75 GiB and require 20 GiB host free space; abort rather than delete
  unrelated or user-owned artifacts. Leave localhost:3000 and other apps untouched.

These are proposed controls to implement and verify in a later scoped change,
not claims that an execution sandbox already exists. Approval would cover only
this one disposable fixture attempt and its temporary guest setup/cleanup, not
production execution, arbitrary code, repository registration or deployment.
