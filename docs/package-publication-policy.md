# Installation-local package publication policy

`devoption_builds::library::publication::Publications` implements a bounded,
owner-only trusted-adapter policy for exact generated Cargo candidates under
[#385](https://github.com/devoption/devoption/issues/385). It records explicit
publication, project-reader and per-version yank decisions in a separate durable
ledger and checks that policy before returning index lines or archive bytes.

This is an installation-local Rust API with an explicit opt-in
[owner controller adapter](registry-controller.md) for staged candidates. It does
not enable itself, supply candidate ingestion from builds, or wire build runners.
There is no anonymous/public Cargo registry, credential issuance or upstream
fallback. Native callers must authenticate the installation owner, select the
actual consumer project and freshly load/serialize its catalog. `OwnerId` and
`ProjectId` are references, not credentials. The controller derives the owner
from the existing session and enforces project grants; it accepts no client actor
or registry identity. Members and per-member registry roles remain outside this
owner-only profile. Keep #385 and #383 open for remaining consumer integration.

## Review and approval

Open a separate existing private policy directory with the configured owner ID.
Supply a [candidate store](cargo-package-storage.md) and its exact coordinates
when preparing a `Change`:

- `Publish { readers }` approves one exact previously unpublished version.
- `Readers { readers }` replaces that version's explicit project-reader set.
- `Yank { yanked }` changes that version's new-resolution eligibility.

Preparation does not mutate the ledger or grant access. It rechecks retained
candidate bytes and the source project/repository association in the current
catalog. Reader IDs must be existing projects, distinct and bounded by the
catalog's 64-project limit. The source project is always included. An empty
requested reader set therefore publishes privately to the source project only;
there is no wildcard or implicit workspace-wide reader grant.

The non-cloneable, process-local review exposes the exact candidate and proposed
change to its trusted adapter. The adapter should show bounded coordinate,
source-revision, checksum/size and reader metadata for human review, keeping
private source bytes out of automatic browser responses. A source-plan binding
still does not attest build approval/success. Publication grants no code execution.

Explicit approval consumes the review by value, including on failure. It expires
after five minutes and binds the configured owner, policy directory/lock inode,
complete supplied catalog, current ledger head, source provenance, entire
candidate fingerprint and full proposed action. Approval rechecks those inputs
and current candidate bytes before appending. A changed catalog (even an unrelated
rename), another intervening policy change, another ledger, expired review or
wrong owner refuses the action. Reviews cannot be serialized across a restart.
The [controller adapter](registry-controller.md) supplies bounded request
admission, Host/Origin/CSRF/session checks and current catalog serialization for
these methods. Other adapters must provide equivalent boundaries.

## Immutable versions and namespace ownership

Publishing an occupied version always conflicts, including an identical repeat;
it never overwrites bytes or silently changes readers. After an uncertain reply,
inspect current authorized state before preparing another action. Exact archive,
index and source identities remain bound through the complete candidate digest.
Read access and yanks change through separate explicit reviews and append-only
events, not by modifying candidates. Each new version needs fresh publication
approval; changing one version's readers/yank state does not change other versions.

The first publication binds a logical registry ID to its exact index URL. Every
later publication in that ledger must preserve that binding. Within a registry,
a case-folded package namespace with underscores normalized to hyphens remains
bound to its original exact spelling, source project and source repository across
versions. A new version cannot take over another project's namespace, change
spelling through an alias, or redirect the logical registry to another index URL.
Namespace transfer and registry rebinding are unsupported, requiring a future
separately reviewed lifecycle rather than overwriting historical records.

## Project reads, revocation and yanks

Every index/archive acquisition requires the configured owner reference, the
actual consumer project and fresh catalog state. Both index and archive paths
recheck explicit readers, project existence, current source association, exact
registry/coordinate spelling and full retained candidate integrity. Unknown and
unauthorized coordinates share the same denial. Missing/corrupt candidates return
unavailability/integrity errors without serving stale metadata or contacting an
upstream. Index access alone grants no transferable archive capability.

`index_line` returns metadata with the currently approved `yanked` flag. A yank
prevents Cargo from selecting that version for a fresh resolution; it does not
change its immutable archive. `archive` requires the exact expected archive
checksum from the admitted lock/acquisition request and continues serving a yanked
version to an authorized project. This preserves old locked builds. An unyank is
another explicit approval. Removing a reader blocks its later index/archive
calls even if it fetched metadata earlier. It cannot retract bytes already
acquired or cached. Runners must still receive only freshly authorized, verified
materializations; this API does not police a caller's existing local cache.

`index_entry` returns a complete newline-delimited Cargo index entry for one
canonical lowercase index path and exact registry binding, filtered to the
requesting project's currently approved versions. Staged/unpublished versions
and versions granted only to other projects are omitted. Yanked versions remain
present with their current flag so previously locked builds can still acquire
their unchanged archives. A reader revocation immediately removes that version
from subsequent entries. If no versions remain, unknown and unauthorized requests
share denial. Case/path/hyphen-underscore aliases and registry URL rebinding do
not select a different namespace.

Entries use deterministic lexical version ordering; Cargo evaluates SemVer and
requirements independently of line order. Every included version rechecks source
association and the full retained candidate fingerprint. A missing included
candidate or corrupt/unsafe store inventory refuses the whole response, including
when corruption is in a hidden or unpublished candidate. No valid prefix or stale
metadata escapes. Whole entries are limited to 1 MiB, and the existing event limit
also disables this acquisition path. Internal streaming retains only approved
index rows plus one candidate at a time after validating the store inventory,
avoiding repeated full-store scans or retaining every archive in memory.

The caller must still authenticate the owner and serialize fresh catalog/policy
observations. An index entry is neither a transferable archive credential nor a
complete dependency closure, materialized vendor bundle or Cargo transport setup.
The [controller contract](registry-controller.md#complete-project-index-entry)
defines its opt-in authenticated HTTP acquisition route.

The owner-only `status` observation reports exact coordinates, source revision and
fingerprint, archive checksum/size, readers, yank flag and event budget after
rechecking retained candidate integrity. It grants no consumer access and fails
visibly for missing or corrupt state. Use these observations to distinguish a
committed action from an uncertain reply before preparing another review.

## Ledger, bounds and failure behavior

The private descriptor-anchored engine is shared with source/candidate staging,
using its own lock, pending name and `.policy` record domain. It requires an
existing absolute directory with no symlink components, mode `0700`, owned by the
current non-root OS user; records/lock must be private owned `0600` regular files.
Do not mix it with source staging, candidates, checkouts or build scratch. Opening
an invalid directory can leave only its newly created private lock file. Exclusive
nonblocking locking and descriptor/inode checks apply to every operation.

At most 128 events are retained, including publication, reader and yank changes;
each is bounded to 16 KiB. There is no eviction, deletion or compaction/reset API.
At full capacity, new changes and index/archive acquisition fail closed,
avoiding a reader set that can still acquire bytes but cannot be revoked.
Owner status remains available with the exact current decision and event budget;
reviews expose the remaining budget after their proposed action. Separate reviewed
ledger maintenance is required before acquisition can resume. Canonical
versioned JSON and an outer SHA-256 bind each event, including its sequence,
previous-event digest, owner, exact coordinates, complete candidate fingerprint,
source project/repository and operation. Opening/reading reconstructs state from
the entire contiguous chain and rejects invalid transitions. Review retains one
bounded candidate and catalog; a service adapter must bound concurrent
reviews. The native API does not supply HTTP rate/concurrency controls.

Events use the existing sync/pending/nonreplacement-link/directory-sync commit
sequence. A partial or uncertain append blocks subsequent ledger reads, including
after restart; it never becomes an empty registry or inferred approval. Preserve
pending evidence. Candidate-store recovery does not repair policy decisions.
There is no automatic startup repair, pending-file discard or ledger reset.

### Fresh review of an interrupted decision

The trusted owner adapter can explicitly complete a **complete** pending policy
record with `Publications::review_recovery`, followed by
`Publications::approve_recovery`. Close ordinary ledger handles first; inspection
and approval each acquire the same nonblocking exclusive ledger lock. Recovery
is a new owner decision to complete the exact pending action, not an inference
that its original approval or a saved UI token remains valid.

Inspection validates the entire committed prefix and the pending event's exact
sequence, previous hash, configured owner, action and candidate fingerprint.
It rechecks retained candidate bytes, source project/repository association and
all reader projects against the freshly supplied catalog. A pending publication,
reader grant, revocation or yank must still be a valid next transition. A pending
hard-linked final record is excluded from the committed prefix only after its
inode and bytes match the pending name. No record is changed during preparation.

The opaque non-cloneable `RecoveryReview` exposes `decision()` for the exact
candidate, proposed change, sequence and remaining capacity, and
`final_link_present()` for the observed commit boundary. Its five-minute lifetime
starts at this fresh inspection. It binds the full catalog, exact pending event,
prior ledger head, directory/lock identity, pending inode and link state. It
cannot be converted into a normal publication approval or serialized as authority.
The adapter must show the exact action, including any grants/revocations and
capacity consequences, and authenticate a new explicit owner confirmation.

Approval reopens and revalidates all observations under the exclusive lock and
holds that same lock through completion. It syncs a complete pending file,
creates the final link without replacement if needed, syncs the directory,
removes only the matching pending name and syncs again. If the final link already
exists, it must be the same inode with exactly two links. Recovery preserves the
original event, action, sequence, candidate bytes and ledger prefix; it does not
append a replacement decision or reset capacity. A pending revocation therefore
completes that revocation instead of exposing the older reader set. Completing
the last event still leaves acquisition blocked by the full-capacity rule.

Wrong owner, stale/expired review, changed catalog/candidate, copied ledger,
replaced pending inode, changed link state, invalid transition, partial/corrupt
record, unknown reader, broken prefix, independent occupied destination or unsafe
storage refuses recovery and retains evidence. If the complete candidate store
itself needs recovery, resolve that separate exact-candidate operation first;
there is no fetch/regeneration fallback in policy recovery. Partial evidence and
conflicts need separate inspection; no generic deletion/repair API is supplied.

Approval consumes its review even on failure. If recovery itself is interrupted,
a complete pending record can receive another fresh recovery review. If the
pending name is already absent, do not infer success or retry a stale approval:
ordinary `Publications::open` and owner `status` must establish the actual decision
and verified candidate. Fresh catalog loading, authenticated owner input and
bounded request/concurrency controls remain duties of the service adapter.
The ledger lock serializes policy writes only: the adapter must also hold its
catalog/authorization gates through approval and acquisition, preventing a supplied
catalog snapshot from racing with an access or project change.
The native API itself neither enables controller endpoints nor restores policy
backups; controller opt-in and constraints are documented separately.

The OS owner and local filesystem are trusted. Hashes detect corruption and broken
chains; they do not prevent an administrator from deleting/rolling back the entire
ledger or its tail. Backups must retain candidates and policy together. Restoring
old reader/yank decisions can regrant access, so keep access disabled until an
operator reconciles policy against authoritative decisions. No filesystem
power-loss, network-filesystem durability or anti-rollback guarantee is claimed.
No credentials are stored in candidates, reviews, events or returned bytes.

## Validation and remaining integration

Workspace tests exercise preparation without publication, wrong-owner refusal,
default-private/project grants and revocation, stale/expired/cross-ledger review,
immutable versions and namespaces, yanks/unyanks and locked checksums, restart,
corruption/gaps/pending evidence, missing candidates and full event capacity.
Existing source/candidate safety and interrupted-write tests cover the shared
engine. Compile-fail coverage prevents constructing normal/recovery reviews
outside their modules. Recovery tests cover both complete commit boundaries,
publication/grants/revocation/yanks, stale/changed/unsafe evidence, current catalog
and candidate refusal, lock ownership and full-capacity preservation. A disposable
child exits without Rust writer cleanup before/after the final link; its parent
performs a fresh owner recovery review and verifies that a revoked reader stays
denied. These are process/fault-injection checks, not power-loss evidence.

The explicit trusted Cargo fixture now passes generated artifacts through both
candidate and publication APIs before seeding disposable offline sources:

```sh
cargo test -p devoption-builds --lib generated_cargo_packages --locked --offline -- --ignored --nocapture
```

It uses one library-source project and two distinct consumer-project grants,
checks denial before publication and for an ungranted project, builds both native
consumers, approves a new version, refuses fresh resolution after an old-version
yank, rebuilds that version with its retained lock in a fresh cache, and checks
revocation and missing/corrupt dependency refusal. All publication records are
synthetic disposable fixture state; no real package or registry is published.
This is policy/Cargo compatibility evidence, not browser authentication, OS network
isolation, a running registry or platform-hosted/WASIp2 consumer acceptance.

The [opt-in controller](registry-controller.md) now supplies authenticated staged
candidate staging/review/acquisition/recovery controls and complete project-filtered
per-package index entries. Remaining #385 work includes Cargo transport/configuration,
complete verified dependency/vendor materialization and service/runner wiring
with actual platform consumers. Installing, globally serving or publishing real
packages, production credentials and access-policy changes require separate
explicit authorization. Public/global distribution is never inferred from local
adapter availability.
