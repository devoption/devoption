# Read-only reviewed-directory verification

[#198](https://github.com/devoption/devoption/issues/198) adds
`devoption_builds::reviewed_directories::ReviewedDirectories`, a Unix filesystem
check for [typed review-context](review-context.md) directory observations. It
opens existing directories, retains private handles, and compares current identity
and permission metadata with the reviewed values. It creates, repairs, writes and
deletes nothing. Ordinary reads can update access times.

There is no production caller, HTTP route, job-directory provisioning, cleanup,
process execution or runner-event issuer. This checks one part of the external
record required by #17; it does not make a complete review context verified or
approve a workload. Approval issuance is not wired to this verifier yet.

## Supported layout

The seven roles must describe one existing private Work tree. Source, Home,
CargoHome, Target, Temporary and Artifact are distinct descendants of Work and
cannot be ancestors of one another. Intermediate private directories are allowed.
Every role must be owned by the current non-root effective OS user; the context's
execution UID must also match that user. The broader typed model can describe
other layouts, but this verifier deliberately rejects separate artifact roots or
multi-account runners. Supporting either needs a separately reviewed implementation.

Paths retain the context's absolute normalized UTF-8 limit of 4096 bytes and have
at most 64 components, including the Work path. Work's ancestors need not be
private or owned by the caller, but no symlink component is followed. From Work
downward, every traversed directory must be on Work's device, owned by the current
account and have exactly mode `0700` or `0500`; no group/other or special bits.
The final role's mode must equal its reviewed mode, not merely be private.

The API assumes explicit trusted-local selection of these paths. It must not be
mounted as an arbitrary browser path inspector. It neither chooses defaults nor
registers real application directories.

## Open, retain and recheck

`ReviewedDirectories::open(&ReviewContext)` performs component-by-component
`openat` traversal with `O_DIRECTORY`, `O_NOFOLLOW` and `O_CLOEXEC`, anchored first
at `/` and then at the retained Work descriptor. Directory-only opens reject files
and special entries. Each final role must match the reviewed device, inode, owner
and permission bits and still have a nonzero link count. Role paths and identities
are already unique in the typed context. The verifier retains all seven handles
and performs a second complete check before returning.

`revalidate()` checks each retained object, reopens the absolute Work binding
without following symlinks, and walks each role again relative to the retained Work
handle. It compares the current name bindings with the original observations,
then checks Work and the pinned metadata again. Replaced paths, missing/unlinked
objects, symlinks, changed owners/modes, or changed filesystem devices fail closed.
Errors are fixed categories without private paths or raw OS error details. There
is no Debug/serialization interface and no public descriptor getter. Drop only
closes handles; it cannot remove the caller's directories or preserved replacements.

This differs from directory creation or normalization: the verifier never resolves
an unsafe path into an acceptable alternative, changes permissions or adopts a
replacement. On failure the future caller must invalidate approval and stop
admission; automatic repair is outside this API.

## Limits and future integration

These are point-in-time observations, not an atomic filesystem snapshot or an
isolation boundary. A trusted same-user process can rename objects, change
permissions and restore them between checks. Retained descriptors prevent simple
path rebinding from silently changing the retained object; they cannot prevent
writes or make a later path-based open safe. A future runner must operate through
reviewed capabilities, exclude other writers, and recheck before admission.

ACLs, inherited writer handles, bind-mount aliases, network filesystem semantics
and filesystem content are not inspected. Device/inode equality is not proof of
mount isolation. No hard cross-filesystem integration test mounts a filesystem;
a focused test exercises device mismatch using an intentionally different allowed
device number. A future supported runner must validate its actual mount layout.
Directory-only syscalls avoid reading special files but there is no filesystem I/O
deadline or protection from a stalled filesystem.

The verifier does not check executable hashes, original repository exclusion,
dependency contents, Cargo configuration, environment application, available disk
space or resource enforcement. Source mode `0500` is not a substitute for read-only
mounts. Those requirements remain in the [execution trust contract](execution-trust.md).
No authenticated review producer, approval/admission transaction or runner is added.

## Validation

Run `cargo test -p devoption-builds --locked --offline reviewed_directories` as an
unprivileged Unix account, plus the [workspace checks](development.md#checks).
Temporary fixtures exercise successful read-only retention/drop, observed-vs-claimed
identity/mode/owner checks, layout/depth limits, symlink ancestors and leaves,
root/leaf replacement, unlinking, permission changes, private intermediates,
device mismatch and missing/regular-file rejection. Linux CI additionally checks
FIFO rejection using rustix mkfifoat (unavailable on macOS). No real job tree or workload
is touched. The implementation reuses the already pinned rustix version and adds
only that existing package to this crate's dependency list.
