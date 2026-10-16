# Private bounded build history

[#216](https://github.com/devoption/devoption/issues/216) adds
`devoption_builds::history::History`, a Unix store for job summaries. It has no
production caller, HTTP route, scheduler, log storage or artifact-byte access.
Persisted records are historical claims, never restored approval or execution
capabilities. The application does not yet create this store.

## Opening and writing

`History::open(path)` requires an explicitly selected existing absolute normalized
UTF-8 path (4096 bytes, at most 64 components). Every component is opened without
following symlinks. The final directory must be owned by the current non-root
account with exact mode `0700`. Opening creates only `build-history.lock`, mode
`0600`, and takes a nonblocking exclusive flock for the handle's lifetime. Another
cooperating writer gets Busy. Directories are never created or repaired.

Directory and lock descriptors remain pinned. Every operation checks their current
ownership/mode, link count and name bindings. Lock and record files must be regular,
single-link, current-account-owned `0600` files. Nonblocking no-follow opens reject
special files before content reads. Directory ancestors' ownership/ACLs are not
verified; explicit trusted-local selection is required, not an arbitrary HTTP path.

`record(&BuildJob)` persists a queued attempt first. Updates must retain attempt,
project, repository, commit and source identity and follow the job lifecycle's
permitted edges; Interrupted is terminal. Identical repeated observations are
idempotent. No terminal outcome can be replaced. A summary does not contain the
complete plan, so these identity checks cannot replace exact approval-plan binding.
There is no general public record constructor, state setter or history-to-plan API.

At most ten attempts are retained, including unfinished attempts. Capacity rejects
new IDs without eviction. This first store has no deletion, seven-day expiry,
completed-job retention policy or cross-store uniqueness allocator; it fails closed
at capacity. Those remain required before production integration.

## Format and publication

`build-history.bin` uses the nine-byte magic/version `DVHISTRY` followed by byte
`01`, a one-byte count, that many 161-byte records, then SHA-256 of all preceding
bytes. Maximum size is 1652 bytes. Each record contains, in order:

- Attempt, project and repository IDs: 16 bytes each.
- Exact commit: 40 lowercase ASCII hex bytes, nonzero.
- Source snapshot digest: 32 bytes.
- State tag: one byte (Queued 0, Running 1, Succeeded 2, preparation/exit/timeout/
  terminated/artifact failures 3–7, Cancelled 8, Interrupted 9).
- Artifact digest and big-endian size: 32 and 8 bytes. Non-success states require
  all zeros; success requires 1 byte–4 MiB. These are metadata claims, not artifact proof.

Duplicate IDs, malformed/unknown fields, truncation, oversized or trailing data and
checksum mismatches fail; they never trigger an empty reset. Missing record alone
means empty history. The checksum detects accidental corruption, not malicious
same-account rewriting. There are no credentials, absolute source/tool paths, source
bytes, executable plans, approvals or logs in this format. Records omit Debug.

Reads are bounded and compare device/inode, size, owner/group, mode, link count and
change/modification timestamps before/after reading and against the current name.
Writes exclusively create `.build-history.pending`, write a complete snapshot,
sync the file, recheck pending/directory/lock bindings, rename atomically over the
record and sync the directory. Only the fixed history filename is replaced.

A publication error poisons that handle. A partial/stale pending file is preserved
for explicit inspection, never adopted or silently removed. A post-rename directory
sync failure can mean the new record is visible but durability is uncertain: reopen
and inspect, do not reset. Reads ignore pending files; a future write refuses to
replace an existing pending file. These semantics rely on trusted local filesystem
and cooperative writers, not an atomic defense against malicious same-user races,
untrusted ACLs, stalled I/O or faulty storage. No filesystem deadline is enforced.

## Explicit restart recovery

`recover_interrupted()` atomically converts all saved Queued/Running summaries to
Interrupted and returns the count changed. Terminal summaries remain unchanged;
a second call changes nothing. Opening alone does not recover or rewrite records.
A future startup adapter must call recovery before admission, invalidate approvals,
validate historical identities, and reconcile surviving processes/writers.

Recovery cannot reconstruct a BuildJob, RunnerEvent or approval, restart a process,
prove child cleanup, or authorize serving a saved artifact hash. Completed history
may be displayed only through an authenticated future UI; actual artifact access
still requires independent provenance and retained-byte verification.

## Validation

Run `cargo test -p devoption-builds --locked --offline history::tests` and the
[workspace checks](development.md#checks). Disposable fixtures cover reopen,
interrupted recovery, terminal preservation, writer exclusion, rebinding, unsafe
files, symlinks/hardlinks, corruption, structural checks with a valid checksum,
capacity, stale pending files and injected partial-write failure. Linux also tests
FIFO rejection. Synthetic running/success metadata in codec tests is not execution
evidence. No real instance history or user repository is changed.
