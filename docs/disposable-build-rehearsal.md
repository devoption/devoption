# Disposable fixture build: observed result

On 2026-10-13, [#131](https://github.com/devoption/devoption/issues/131) exercised
one separately approved build under the
[disposable runner profile](disposable-runner-proposal.md). **Cargo exited 0,
but the overall rehearsal failed during artifact verification.** No artifact
checksum or size receipt was produced. The artifact was neither executed nor
exported, and the disposable VM is stopped.

This was a temporary harness for a trusted fixture, not a production runner or an
application build job. It issued no `BuildJob` event and added no runtime route,
repository registration, release or deployment capability. The independent
[Linux CI fixture proof](offline-fixture-build.md) has its own artifact evidence;
its checksum cannot substitute for this attempt's missing receipt.

## Inputs and preparation

The source, exact lockfile, registry index and dependency archive came from reviewed
commit `0393454b28bcffce4e54af9a17f5582e056f407d`
([PR #128](https://github.com/devoption/devoption/pull/128)). Input hashes were
checked before execution. The pinned `itoa` 1.0.18 archive's SHA-256 was
`8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682`.
No registered or user repository was consumed.

The existing Ubuntu 24.04.5 amd64 guest supplied Rust/Cargo 1.99.0 and GCC 13.3.0.
No tools or dependencies were downloaded or installed. Because the installed
toolchain was private to the guest's existing user, the harness made a temporary
read-only copy for the transient build identity, verified every copied regular
file against its original, and left the original permissions unchanged. Only an
owned, rebuildable host debug cache was cleared to make disk-budget room.

An initial transient-unit request was rejected before the runner or Cargo started.
The systemd 255 transient API required an empty address-family allow-list instead
of the unit-file literal `none`. After correcting that encoding, the runner
verified socket denial before invoking Cargo. Exactly one Cargo build ran; the
artifact failure did not trigger another attempt.

## Observations from the running job

The runner checked these controls from inside the restricted process before
launching Cargo:

| Control | Observed value |
| --- | --- |
| Identity | Transient UID distinct from root and the existing guest user |
| Privileges | No effective capabilities; `NoNewPrivileges` set |
| Network | Separate network namespace; IPv4, IPv6 and Unix socket creation each rejected with `EAFNOSUPPORT` |
| Memory / swap | Cgroup `memory.max=536870912`, `memory.swap.max=0` |
| CPU | Cgroup `cpu.max=100000 100000` (one CPU quota) |
| Processes | Cgroup `pids.max=64` |
| Scratch | Fresh 268,435,456-byte tmpfs, writable and `noexec` |
| Inputs / toolchain | Read-only mounts; opening a source input for writing rejected |

Cargo received a cleared environment with the fixed toolchain, owned working
directories, offline source replacement, one compile job and empty compiler
wrappers. It was launched as a direct argument vector:

```text
cargo build --release --locked --offline --package offline-fixture --bin offline-fixture
```

Cargo exited 0 and reported completing the release compile in 3.92 seconds. Its
combined captured output was 237 bytes, below the 1 MiB capture limit. This is
compiler-reported build time, not a measured total runner duration.

The transient service was configured for a 60-second runtime limit and a
five-second stop grace with complete-cgroup termination. Timeout, cancellation,
limit exhaustion and log-overflow paths were not exercised. The post-exit
`systemctl show` snapshot was taken after systemd collected the failed unit and
returned unloaded-unit defaults; it does **not** verify the live configuration.
The observations above come from the running process's checks instead. No peak
memory or total elapsed-time receipt was retained.

## Artifact failure and evidence limits

After Cargo exited, the harness opened the expected output with `O_NOFOLLOW` and
failed a combined assertion requiring a regular file, exactly one hard link, and
a size between 1 byte and 4 MiB. It did not record those metadata values before
the assertion. The exact failing condition is therefore unknown; normal Cargo
hard-link behavior is only a possible explanation, not an observed diagnosis.

The extra single-link condition went beyond the approved profile's regular-file,
no-symlink and size requirements. No bytes were hashed after that assertion, and
the final input-hash recheck was not reached. Read-only inputs were verified
before execution, but an after-build comparison cannot be claimed.

The outer cleanup driver returned success while the inner receipt recorded the
failure. The recorded inner result, not the SSH/cleanup exit status, determines
the rehearsal outcome. A future harness should record each artifact predicate
separately, propagate the inner result, and capture live unit properties before
collection. Any revised artifact policy needs to remain within the reviewed scope.
Another disposable build requires a new explicit approval; this attempt was not
repeated and does not close the artifact-verification gap in #131.

## Diagnostic follow-up without another build

The pure [metadata diagnostic helper](../scripts/rehearsal_artifact_metadata.py)
now reports mode, link count, byte size, the size limit and every failed predicate
before a caller decides whether to reject. It preserves the exact failed
rehearsal's acceptance rule, including the single-link condition; it neither
weakens isolation nor changes artifact policy. It performs no I/O or execution.

Temporary-file tests cover regular files, hard links, combined empty/hard-link
failures, directories, symlinks and the inclusive 4 MiB boundary. These establish
diagnostic behavior, not which condition failed in the deleted VM artifact.
The retained attempt's logs and harness are unchanged.

A future, separately approved harness can retain the report before rejecting:

```python
report = describe_artifact_metadata(os.fstat(artifact.fileno()))
receipt["artifact_metadata"] = report
if not report["metadata_accepted"]:
    raise RuntimeError("artifact metadata rejected: " + ", ".join(report["failed_checks"]))
```

The helper has not been run in the VM and is not integrated with a production
runner. Passing its metadata gate does not prove safe path selection, stable
bytes, a checksum, or artifact acceptance. Future integration must still persist
failure receipts, propagate the inner failure and verify the live unit controls.

## Cleanup and retention

Cleanup verified that the complete service cgroup was empty, the transient unit
and identity were removed, and the job tree, temporary toolchain copy and guest
harness were deleted. The original toolchain retained its original permissions.
The VM and the host disk monitor were then stopped. No executable was exported.

The one-second host allocation monitor recorded a peak of 3,444,887,552 bytes
(3.21 GiB), below the 3.75 GiB stop threshold and 4 GiB cap. After cleanup, allocated
usage was 2,236,207,104 bytes (2.08 GiB); these are sampled values, not an assertion
about every instant. Bounded logs and metadata remain in the existing ignored
rehearsal directory. Existing VM images, unrelated apps and user data were retained.
