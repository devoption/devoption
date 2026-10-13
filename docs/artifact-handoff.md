# Disposable artifact handoff: independent staging

[#131](https://github.com/devoption/devoption/issues/131) adds a Unix
[rehearsal helper](../scripts/rehearsal_artifact_handoff.py) and disposable file
tests. It copies bounded source bytes into a new private inode, then validates
and hashes the destination. No VM build, process launch, execution/export,
application route or `BuildJob` event is added. The actual VM artifact remains
rejected as recorded in the [diagnostic repeat](disposable-build-repeat.md).

## Why Cargo can produce two links

At the installed Cargo commit `5f94df4789f005f9a352888e8355ffc645b7ed0e`,
[`link_targets`](https://github.com/rust-lang/cargo/blob/5f94df4789f005f9a352888e8355ffc645b7ed0e/src/compiler/mod.rs#L675-L692)
places an output from the dependency directory at its profile-level binary name.
Its [`link_or_copy` helper](https://github.com/rust-lang/cargo/blob/5f94df4789f005f9a352888e8355ffc645b7ed0e/crates/cargo-util/src/paths.rs#L562-L643)
uses a hard link on Linux, with a copy fallback. Thus a backing binary under
`release/deps/` and the visible `release/offline-fixture` can share one inode.
This explains a normal two-link layout without making it an acceptable final
artifact. The retained receipt proves the repeat's link count of two; it did not
record the other name, which still must be independently selected and verified.

## Caller prerequisites

The helper is a filesystem operation inside a trusted supervisor, not a runner
or a termination detector. **Before calling it**, the supervisor must confirm the
whole runner cgroup is stopped and empty, prevent restarts and all remaining
writers, and retain the original output filesystem. A process exit code, caller
boolean or directory metadata does not establish those conditions.

The caller supplies an already-open private supervisor-owned parent directory
handle. Source and staging must be distinct pinned children of that same parent
on the same filesystem. The source is owned by the recorded runner UID; staging
is supervisor-owned, empty and mode `0700`. The parent must be mode `0700` and
inaccessible to the runner, with no extra ACL/shared access or untrusted writer
handles. Unix mode checks alone cannot establish that complete trust boundary.

Keep the source and staging inside the same quota-controlled job filesystem.
Reserve room for the at-most-4-MiB copy within the existing 256 MiB scratch limit;
do not move it to an unbounded host directory. The helper does not provision or
increase filesystem, CPU, memory, process, log or elapsed-time quotas.
Integration must keep the job-wide deadline and resource caps in force during
handoff without resetting them after runner termination. Only local Unix
filesystems with the stated ownership/link semantics are in this fixture scope.

## Copy and validation contract

- `PinnedDirectory` retains parent/child descriptors and verifies ownership,
  private modes, same-filesystem identity and the original child-name binding.
  It follows no symlink and never repairs permissions or creates directories.
  The trusted parent descriptor is the authority, not a subsequently reopened
  parent pathname.
- Supply one or two exact bounded relative source names, with at most 16
  components and 1,024 ASCII bytes per path. Empty, absolute, dot/parent, control
  character and overlong components fail. There is no glob, recursive discovery
  or inference of an artifact's identity from a filename alone.
- Each component opens relative to a retained directory descriptor with
  `O_NOFOLLOW`; directories require `O_DIRECTORY`. Files require a private,
  regular, nonempty inode owned by the expected source UID, at most 4 MiB.
  Nonblocking opens avoid waiting on a replaced FIFO. Cross-filesystem traversal
  is rejected.
- All supplied source names must refer to the same inode and to distinct actual
  directory entries. Their count must equal the inode's link count. An omitted,
  duplicate, different-inode or outside-root alias therefore fails. Accepting
  arbitrary hard-linked source files without accounting for their aliases is
  not supported.
- The helper exclusively creates `artifact` in empty private staging, mode `0600`,
  and copies at most 4 MiB in 64 KiB chunks. It never hard-links, renames or deletes
  a source name. It compares retained metadata and path bindings before and after
  copying, including inode, ownership, mode, size, link count, mtime and ctime.
  Growth, truncation, replacement and observed modification fail closed.
- Before hashing, the destination must have a different inode, expected ownership,
  the exact copied size, and the unchanged regular/nonempty/single-link/size gate.
  It is made read-only (`0400`) and synchronized. The helper hashes its actual
  bytes through the retained descriptor, then rechecks destination stability,
  source bindings and the private staging directory. No source inode is accepted
  as the destination simply by relaxing `nlink`.

The context manager yields only byte count, SHA-256 and source link count while
the temporary destination exists. These describe that file at handoff time; they
do not authorize execution, establish source provenance or mark a job successful.
On normal exit or failure, the helper removes only the destination it created,
keeping its inode pinned until cleanup. An observed replacement is preserved and
reported as a cleanup conflict. Exclusive trusted staging access remains required
through named-entry cleanup; metadata rechecks are not an atomic snapshot against
an unrestricted concurrent writer. The supervisor must quarantine and dispose of its owned
staging tree after any failure; it must not treat a leftover file as an artifact.

## Evidence and remaining integration

Twenty-one disposable-file tests prove an independent single-link destination from
two in-root source links, inclusive size limits, rejection of an outside alias,
no symlink following, replacement during open/copy, growth/shrink/rewrite,
destination link or content tampering, short writes/disk failure and cleanup
conflicts, including cleanup while preserving a caller's own exception. No test
compiles or executes an artifact. The existing metadata helper
and its rejection of a two-link final file are unchanged.

A final separately approved VM rehearsal is still needed after reviewed supervisor
integration. The previous service-owned scratch mount disappears with its namespace;
the controller must retain the original filesystem across full runner termination,
verify the stopped state, and pin the exact output aliases before staging. This
slice does not implement that controller or change mount lifetime/permissions.
It does not claim the helper has accepted a real VM artifact or demonstrated the
complete stop/copy/hash/cleanup sequence under the VM's enforced limits.
