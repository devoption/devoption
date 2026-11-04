# Private bounded build history

[#216](https://github.com/devoption/devoption/issues/216) adds
`devoption_builds::history::History`, a Unix store for job summaries. It has no
scheduler or artifact-byte access. Authenticated project pages can read saved
summaries and output through the adapter below. Explicit server startup can open
and recover the store as described below.
Persisted records are historical claims, never restored approval or execution
capabilities. The application never creates its directory.

## Opening and writing

`History::open(path)` requires an explicitly selected existing absolute normalized
UTF-8 path (4096 bytes, at most 64 components). Every component is opened without
following symlinks. The final directory must be owned by the current non-root
account with exact mode `0700`. Opening creates only `build-history.lock`, mode
`0600`, and takes a nonblocking exclusive flock for the handle's lifetime. Another
cooperating writer gets Busy. Dropping the writer explicitly unlocks it, including
when a concurrent subprocess briefly retains an inherited descriptor before exec. Directories are never created or repaired.

Directory and lock descriptors remain pinned. Every operation checks their current
ownership/mode, link count and name bindings. Lock and record files must be regular,
single-link, current-account-owned `0600` files. Nonblocking no-follow opens reject
special files before content reads. Directory ancestors' ownership/ACLs are not
verified; explicit trusted-local selection is required, not an arbitrary HTTP path.

Under [#218](https://github.com/devoption/devoption/issues/218),
`queue(BuildPlan)` generates a 128-bit attempt ID using OS randomness and saves
Queued before returning the new `BuildJob`. It checks candidates against every
retained ID, including terminal attempts, and retries at most sixteen times.
Entropy failure or collision exhaustion returns Randomness without writing;
capacity is checked before requesting entropy. Publication failure returns no job
and retains the poisoned-writer/pending-file behavior described below. If publication
was visible before a durability error, reopen and inspect the saved summary; never
reconstruct a queued job from it.

`record(&BuildJob)` updates only an existing attempt; unknown IDs are rejected.
Updates must retain attempt, project, repository, commit and source identity and follow the job lifecycle's
permitted edges; Interrupted is terminal. Identical repeated observations are
idempotent. No terminal outcome can be replaced. A summary does not contain the
complete plan, so these identity checks cannot replace exact approval-plan binding.
There is no general public record constructor, state setter or history-to-plan API.

At most 65,536 records may be retained simultaneously, including protected and
unfinished attempts. This is an active-storage safety bound, not a lifetime build
count: successful retention removes eligible records and releases slots. The
per-project default is ten completed attempts. Compact per-ID audit records retain
identity/provenance after pruning, so allocation checks both active records and
audit names before returning a new random ID. No small lifetime audit-count bound
is imposed; compact metadata still consumes disk and is not secure erasure.
Never reset, roll back or delete history/audits to reuse attempt IDs. Legacy ten-
and hundred-record stores remain readable; see migration and downgrade limits below.

Allocation does not validate current source/catalog state, consume approval,
serialize single-job admission, prove process cleanup or perform startup recovery.
It can queue more than one attempt up to capacity; production admission remains
separate. The lower-level `BuildJob::queued` constructor is still an in-memory
model API and cannot insert caller-chosen IDs into this history store.

## Per-project retention and automatic cleanup

[#314](https://github.com/devoption/devoption/issues/314) connects the pure
`devoption_builds::retention` planner to private history, execution and Settings.
An authenticated workspace owner can save 1–1000 completed attempts per project,
default 10, on **Project → Settings → Build retention**. The policy is stored in
that history directory, separately for each stable project identity. History must
be explicitly configured; unavailable storage cannot be treated as a saved policy.
Saving explains cleanup and exceptions before the action. Members cannot change it.

All terminal outcomes count, including failed, cancelled and interrupted attempts.
A durable increasing completion sequence orders transitions; input row order and
wall-clock timestamps do not determine new completions. The newest N are retained.
Older queued/running attempts, unconfirmed process cleanup, active promotion and
all saved-release references (including deployed and recovery releases) are
protected in addition to the window. Protected counts can exceed the configured
limit. Interrupted attempts become cleanup-eligible only after startup has stopped
and verified the complete old build unit; recovery never resumes a workload.

Cleanup runs after final execution checkpoints, after replacing a prepared review,
after a setting change and before listening after restart. The controller holds
execution, history and catalog gates in that order. The installed promotion broker
also uses the work root's private `retention.lock`, with nonblocking exclusive
flock, through copying the artifact and saving its immutable release. Cleanup holds
that same lock while observing references and deleting. The strict `retention-references` broker query refuses corrupt manifests,
unfinished promotions and missing deployment/recovery targets. Lookup failures
preserve builds, and every candidate gets a fresh reference check immediately
before removal. Existing local release bytes are never rewritten or removed.

Preparation binds each allocated attempt to its exact direct staging child, root
and child device/inode in private `build-owned-ID.bin` metadata. Cleanup accepts
only that directory, refuses symlinks, hard-linked files, cross-device trees,
unexpected ownership/writable modes and changed identities, and verifies the
whole tree before deleting entries through retained no-follow descriptors. Saved
output must pass checksum and embedded-attempt validation before any staged files
are removed. Cleanup removes staged source, target/compiler files, artifact, receipt, temporary/home/
Cargo files and the saved output checkpoint. Concurrent out-of-band changes by
the trusted platform OS user remain outside the contract.

Before deletion it durably writes `build-audit-ID.bin`, containing the compact
terminal identity/source/artifact claims and completion metadata. History keeps
the original record until removal finishes, then atomically compacts it while
preserving the completion high-water mark. Audit and owned-path metadata remain;
allocation consults audits to prevent reuse. If interrupted after partial tree,
directory or output removal, restart retries from the retained record and binding.
Missing already-removed paths are harmless; replacement identities are refused.
Retention publications use content-addressed private `.retention-DIGEST.tmp`
files: a matching partial prefix can be completed on retry, while unsafe or
unrelated bytes are refused. Other history/output writes keep their existing
poisoned-writer inspection rules.

Settings shows the last cleanup's retained/protected counts, successful removals
and reclaimed file-byte count, plus an explicit incomplete result on failure.
Verified retained/protected counts remain visible when removal fails; counts
that could not be verified are shown as unavailable rather than zero.
Only successful removals contribute bytes; failed or partially removed attempts
claim none. These are file lengths, not a filesystem free-space guarantee. Saving
a policy and successful cleanup are distinct results. Re-saving retries cleanup.
A failed status publication is logged; unavailable status storage is shown as such.

### Legacy data and limits

Version-1 records have no completion clock. Reads establish a deterministic
migration order from existing terminal row positions, without claiming historical
wall-clock order; new terminal transitions follow that high-water mark. The first
history write uses version 2. Existing identity, provenance and log bytes are
preserved. Old binaries cannot read version 2: restore a matching full-state backup
for downgrade rather than resetting data. Policy absence defaults to ten.

Legacy attempts also lack a verified staging-path binding. Cleanup preserves those
attempts/files and reports incomplete cleanup instead of guessing a directory.
This exception can exceed the policy; existing unmanaged/conflicting paths are not
automatically adopted or deleted. New prepared attempts have a binding and are
reclaimed normally. Compact audit metadata has no age-based expiry. The active
record bound and ordinary storage failures still fail closed.

Run `cargo test -p devoption-builds --locked --offline history::` and `retention::`
for policies, migration, completion order, protected references, disk/capacity
reclamation, interrupted retries, unsafe paths and ID/approval single use beyond
the former 64/100 attempt ceilings. The disposable multi-app Linux checker also
builds two unsaved components, verifies older staging/artifact/receipt/output
removal and retained audit metadata, then deploys the saved old release again.

## Format and publication

`build-history.bin` uses the nine-byte magic/version `DVHISTRY` followed by byte
`02`, a big-endian u32 count and u64 completion high-water mark, that many
170-byte records, then SHA-256 of all preceding bytes. The maximum is 11,141,173
bytes. Version 1 with its one-byte count and 161-byte records remains readable. Each record contains, in order:

- Attempt, project and repository IDs: 16 bytes each.
- Exact commit: 40 lowercase ASCII hex bytes, nonzero.
- Source snapshot digest: 32 bytes.
- State tag: one byte (Queued 0, Running 1, Succeeded 2, preparation/exit/timeout/
  terminated/artifact failures 3–7, Cancelled 8, Interrupted 9).
- Artifact digest and big-endian size: 32 and 8 bytes. Non-success states require
  all zeros; success requires 1 byte–4 MiB. These are metadata claims, not artifact proof.

Version-2 records append a big-endian u64 completion sequence (zero only for
unfinished attempts) and a cleanup-confirmed byte (0 or 1). Duplicate completion
sequences, sequences beyond the watermark and inconsistent terminal flags fail.

Duplicate IDs, malformed/unknown fields, truncation, oversized or trailing data and
checksum mismatches fail; they never trigger an empty reset. Missing record alone
means empty history. The checksum detects accidental corruption, not malicious
same-account rewriting. There are no credentials, absolute source/tool paths, source
bytes, executable plans, approvals or logs in this summary format. Records omit Debug.

Reads are bounded and compare device/inode, size, owner/group, mode, link count and
change/modification timestamps before/after reading and against the current name.
Writes exclusively create `.build-history.pending`, write a complete snapshot,
sync the file, recheck pending/directory/lock bindings, rename atomically over the
record and sync the directory. Only the selected internal snapshot filename is replaced.

A publication error poisons that handle. A partial/stale pending file is preserved
for explicit inspection, never adopted or silently removed. A post-rename directory
sync failure can mean the new record is visible but durability is uncertain: reopen
and inspect, do not reset. Reads ignore pending files; a future write refuses to
replace an existing pending file. These semantics rely on trusted local filesystem
and cooperative writers, not an atomic defense against malicious same-user races,
untrusted ACLs, stalled I/O or faulty storage. No filesystem deadline is enforced.

## Output checkpoints

[#220](https://github.com/devoption/devoption/issues/220) adds
`save_output(&BuildJob, &BuildOutput)` and `load_output(JobId)`. Both require a
known saved attempt. Writes require an unfinished saved state, matching summary
identity and the supplied plan's log limit. Subsequent checkpoints must preserve
the limit, each stream's existing byte prefix and sticky truncation. This compares
summary identity, not the full approved plan; callers must still bind the collector
to the actual attempt. It proves neither execution nor authenticity of output.

Load returns `None` for a missing checkpoint, distinct from a saved empty log.
Output restores the same [escaped-text interface](build-output.md), with no public
raw-byte accessor. Terminal history remains readable; writes to it are rejected.
Persist the final checkpoint **before** the terminal summary. These are separate
atomic publications, not a transaction. A crash can leave an older or missing log;
neither presence nor absence establishes EOF, completion or successful execution.
Recovery changes only summaries and preserves the last saved checkpoint.

Each `build-output-<32 lowercase hex attempt ID>.bin` uses `DVOUTPUT` followed by
version byte `01` (nine bytes total), attempt ID (16 bytes), limit/stdout length/stderr
length (three big-endian u32 values), truncation byte (0 or 1), stdout bytes,
stderr bytes, then SHA-256 of everything preceding it. The header is 38 bytes;
maximum file size is 1,048,646 bytes. Lengths share the saved limit of 1–1,048,576
bytes; truncation requires a full buffer. Wrong IDs, versions, lengths, flags,
checksums, trailing bytes and oversized files fail closed. Checksums detect
corruption, not malicious rewriting. The summary format is unchanged.

Logs use the same retained directory/lock, no-follow bounded reads, metadata/name
checks, fixed pending filename, atomic replacement and file/directory sync as
summaries. Corrupt or unsafe existing logs cannot be overwritten. Failed writes
poison the shared writer, leaving uncertain files for inspection. No directory
scan or orphan adoption is performed by log reads/writes. Retention separately
removes eligible output checkpoints. Each retained attempt can keep approximately
1 MiB of logs; protected exceptions and the active-record bound still apply.
Unrelated entries or same-account modifications are outside that bound.

Raw stored logs can contain secrets or private paths. Private filesystem modes
are not redaction or secure deletion. The opt-in Linux integration checkpoints bounded output during a build;
retention removes eligible old checkpoints using the policy above.

## Explicit restart recovery

`recover_interrupted()` atomically converts all saved Queued/Running summaries to
Interrupted and returns the count changed. Terminal summaries remain unchanged;
a second call changes nothing. Opening alone does not recover or rewrite records.
Before future execution admission, integration must invalidate approvals, validate
historical identities, and reconcile surviving processes/writers.

Recovery cannot reconstruct a BuildJob, RunnerEvent or approval, restart a process,
prove child cleanup, or authorize serving a saved artifact hash. Completed history
are displayed through authenticated project pages; actual artifact access
still requires independent provenance and retained-byte verification.

## Explicit server startup

Under [#222](https://github.com/devoption/devoption/issues/222), Unix startup accepts:

```sh
cargo run --locked --offline -- serve --data-dir /absolute/private/owner-data --build-history-dir /absolute/private/build-history
```

The history directory must already exist and satisfy the storage rules above.
The option requires an existing configured owner in `--data-dir`. It is compatible
with either repository-root mode. Omitting it leaves history untouched; there is
no inferred/default path, directory creation or permission repair.

Startup validates owner/catalog/profile storage, opens the selected history and
recovers unfinished summaries before binding HTTP or announcing a listener.
Under [#224](https://github.com/devoption/devoption/issues/224), it checks every
summary, including terminal records, against the current catalog: the project and
repository must exist and the repository must be associated with that project.
All records validate before any recovery write. The retained catalog directory's
cooperating writer lock is held through validation and recovery, then released
before serving; lock acquisition failure prevents startup. History is locked
first, then the catalog. Empty history is valid with an absent/empty catalog;
nonempty history is not. Invalid membership leaves history and logs untouched.
These are startup membership checks only; page reads independently check a fresh
catalog snapshot, and future admission must do so too. These checks do not
authenticate historical claims or establish source freshness, current checkout
binding or a complete approved plan. The server holds the shared history writer
capability until it exits. Another cooperating writer/server fails startup. Unsafe, corrupt
or busy summary storage and failed recovery publication fail startup without
resetting data. Output files are preserved, not eagerly scanned or validated;
detail-page reads use `load_output` checks. Existing pending files are preserved
and block any needed recovery write. A bind failure after recovery can leave the
interrupted summaries saved; recovery is intentionally not rolled back.

History-only opening/recovery does not restore approvals, inspect/kill surviving
processes or delete job directories. With [Linux execution](web-app-builds.md)
explicitly enabled, the server separately stops and verifies the fixed build unit
before history recovery. Restart never resumes a queued/running attempt or
restores approval. An Interrupted label remains a record of lost continuity.

## Authenticated project history

[#226](https://github.com/devoption/devoption/issues/226) connects the configured
store to `/projects/{project}/builds` and
`/projects/{project}/builds/attempts/{attempt}`. The owner must sign in; existing
session, Host/Origin and method protections apply. Responses use `Cache-Control:
no-store`. No mutation, raw-log download, approval, cancellation or artifact route
is added.

The list shows only that project's saved attempts in reverse allocation order,
with repository name, exact commit and recorded state. No timestamp, elapsed-time
or live-process claim is invented. Unconfigured storage, empty history and read
failure are distinct. Existing source checks remain below history.

Detail shows recorded outcome and explanation, expandable attempt/commit/source
identities, and the last stdout/stderr checkpoint. Missing output differs from an
empty saved stream or unavailable/corrupt output. Byte count, truncation and exact
limit-fill states remain visible. Interrupted never means verified cleanup;
Succeeded is a historical claim and does not expose an artifact.

Each request reads a fresh catalog through its retained directory, checks the
project and every displayed repository association, and filters records by project
before selecting an attempt or opening its output. A cross-project/unknown attempt
returns 404 without log content; unavailable storage or stale associations return
503 without mutation. Association checks use one fresh snapshot, not a transaction
against malicious same-user edits. Future execution admission still needs its own
fresh authorization/verification transaction.

The adapter serializes on the shared history capability and catalog gate, in that
order, and moves guards into a blocking task for disk reads, decoding and rendering.
Disconnecting a request does not prematurely release those guards. At most ten
summary rows and one bounded output checkpoint are rendered per request. Large
logs have a bounded scroll area and wrap within it. No automatic polling or live
streaming is implemented. Controls/bidi become visible escapes via `display_text`;
the UI then HTML-escapes all text at the final sink, including names and log markup.
Escaping is not redaction: authenticated owners can read secrets printed into logs.

## Validation

Run `cargo test -p devoption-builds --locked --offline history::tests` and the
[workspace checks](development.md#checks). Disposable fixtures cover reopen,
interrupted recovery, terminal preservation, writer exclusion, rebinding, unsafe
files, symlinks/hardlinks, corruption, structural checks with a valid checksum,
capacity, stale pending files, injected partial-write failure, entropy failure,
collision exhaustion, fresh retry IDs after recovery and unknown-ID rejection.
Output tests cover reopen/recovery, escaping, maximum exact fill, append-only
updates, swapped-attempt rejection and unsafe-file preservation.
Linux also tests FIFO rejection for summaries and logs. Synthetic running/success metadata in codec tests is not execution
evidence. No real instance history or user repository is changed.

`cargo test -p devoption-cli --test build_history --locked --offline` starts only
disposable loopback servers. It checks recovery before listen, lock lifetime,
restart idempotence, preserved logs/terminal records, omission and rejected startup.
Catalog tests cover unknown/unassociated identities, mixed valid/invalid history,
terminal foreign records, busy/corrupt/missing catalogs and empty-history startup.

`cargo test -p devoption-server --lib --locked --offline session_tests::history`
covers authentication/logout, project scoping, stale catalog associations, invalid
IDs, missing/corrupt logs, unsafe Host/mutation methods, HEAD and output escaping.
The [screenshot guide](screenshots/README.md) describes synthetic history fixtures;
those records are display tests, not evidence of successful builds.
