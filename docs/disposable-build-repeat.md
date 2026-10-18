# Disposable diagnostic repeat: artifact rejected

On 2026-10-13, a separate approval under
[#131](https://github.com/devoption/devoption/issues/131) allowed exactly one repeat
of the [initial disposable rehearsal](disposable-build-rehearsal.md), using the
merged metadata diagnostic helper. **Cargo exited 0, but the unchanged artifact
gate rejected the output solely because it had two hard links.** No acceptance
predicate or isolation setting was weakened, and no additional build followed.

## Exact result

The expected output was opened with `O_NOFOLLOW`. The helper recorded its metadata
before the existing gate made its decision:

| Predicate | Observed value | Result |
| --- | --- | --- |
| Regular file | Mode `0o100700` | Pass |
| Exactly one hard link | Link count `2` | **Fail** |
| Nonempty | `445888` bytes | Pass |
| At most 4 MiB | `445888 <= 4194304` bytes | Pass |

The receipt's sole failed predicate was `single_link`; the runner reported
`RuntimeError: artifact metadata rejected: single_link`. The driver propagated
failure with exit status 1 after cleanup. Cargo itself exited 0, reported 4.01
seconds for the compile, and produced 237 bytes of captured output.

This establishes the repeat's exact rejection, not a successful artifact receipt.
The gate stopped before hashing, so there is no artifact checksum. No executable
was run or exported. The location of the other hard link was not recorded. The
first attempt's metadata remains unknown; the repeat does not retrospectively
prove which predicate failed then.

## Unchanged scope and observed controls

The five fixture inputs were byte-identical to the initial attempt, from reviewed
commit `0393454b28bcffce4e54af9a17f5582e056f407d`. The diagnostic helper came unchanged
from merge commit `cb95a75cbd7b5811de1278acf8fd96a9b88f16a4`
([PR #132](https://github.com/devoption/devoption/pull/132)); its SHA-256 was
`e8abdee0edbc824eae838bb1e196d2124064a6af1d26c493c097b0d4b94f34c8`.
The existing guest's Cargo, Rust and linker hashes matched the initial attempt.
Nothing was downloaded or installed, and no user repository was accessed.

The service property set was identical to the first attempt. Inside the running
job, checks again verified a non-root transient identity, no effective
capabilities, `NoNewPrivileges`, read-only input/toolchain mounts, a fresh 256 MiB
scratch tmpfs, a one-CPU quota, 512 MiB memory, zero swap and a 64-process limit.
IPv4, IPv6 and Unix socket creation each failed with `EAFNOSUPPORT` in the private
network namespace before Cargo ran. The cleared environment, fixed offline build
command, 1 MiB log bound and 4 MiB artifact limit were unchanged.

This time the driver retained a live, loaded/running unit snapshot confirming
the resource settings, private network, read-only bindings, 60-second runtime
limit and five-second complete-cgroup stop policy. This is separate from the
unloaded defaults returned after systemd collected the failed unit. Timeout,
cancellation, limit exhaustion and log overflow were still not exercised.

The failure receipt also verified that all input hashes remained unchanged after
the build. It recorded a cgroup memory peak of 37,281,792 bytes and 4.511 seconds
inside the runner; that duration excludes VM startup and toolchain preparation.

## Cleanup and remaining work

Cleanup confirmed an empty service cgroup, removed transient unit and identity,
and deleted the owned job tree, read-only toolchain copy and temporary guest
harness. No job mount remained in the management namespace. The original
toolchain retained its permissions. The VM was stopped, ending its remaining
mount namespaces, and the host disk monitor was stopped afterward.

The one-second allocation monitor sampled a peak of 3,405,733,888 bytes (3.17 GiB)
and 2,777,976,832 bytes (2.59 GiB) after cleanup. Both were below the 3.75 GiB stop
threshold and 4 GiB cap. No host cache cleanup was needed. Logs and metadata remain
in the existing ignored rehearsal directory; unrelated apps and user data were
retained. The approved disposable repository root remained empty.

At the end of this repeat, issue #131 remained open: the diagnostic gap was
resolved, but there was no accepted artifact or checksum from this runner. Any change to artifact
selection or the single-link policy needs explicit review; another VM build needs
separate approval. No production runner, job event, release or deployment was added.

The subsequent [artifact handoff slice](artifact-handoff.md) tests an independent
private copy without changing this result or running another VM build. It retains
the destination single-link requirement. The later selected Wasm workflow does not
retroactively validate this native experiment; see its
[superseded disposition](single-node-acceptance-status.md#native-experiment-131).
