# Installation-local package publication policy

`devoption_builds::library::publication::Publications` implements a bounded,
owner-only trusted-adapter policy for exact generated Cargo candidates under
[#385](https://github.com/devoption/devoption/issues/385). It records explicit
publication, project-reader and per-version yank decisions in a separate durable
ledger and checks that policy before returning index lines or archive bytes.

This is an installation-local Rust API. No controller route, CLI command,
installer or build runner opens this ledger yet. There is no anonymous/public
registry, HTTP authentication endpoint, credential issuance or upstream fallback.
The caller must authenticate the installation owner, select the actual consumer
project and freshly load its catalog. `OwnerId` and `ProjectId` are references,
not credentials: taking them from a request body/header does not authenticate a
caller. Existing browser authentication is not wired into this API. Members and
per-member registry roles are outside this owner-only profile. Keep #385 and
parent #383 open until service and consumer integration is delivered.

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
A trusted HTTP adapter will also need its own request limits, Origin/CSRF checks
and current authenticated session before it can safely call these methods.

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
bounded candidate and catalog; a future service adapter must bound concurrent
reviews. The native API does not supply HTTP rate/concurrency controls.

Events use the existing sync/pending/nonreplacement-link/directory-sync commit
sequence. A partial or uncertain append blocks subsequent ledger reads, including
after restart; it never becomes an empty registry or inferred approval. Preserve
pending evidence. This policy API intentionally exposes no automatic/in-place
ledger recovery or pending-file deletion operation yet. Candidate-store recovery
does not repair policy decisions. Keep registry access disabled during inspection
until a separately implemented and reviewed ledger-recovery procedure resolves
its exact approved decision. That remaining recovery integration stays in #385.

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
engine. Compile-fail coverage prevents constructing a review outside the module.

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

Remaining #385 work includes authenticated controller review/actions and scoped
acquisition, approved index aggregation/serving, explicit ledger recovery,
complete verified dependency/vendor materialization and service/runner wiring
with actual platform consumers. Installing, globally serving or publishing real
packages, production credentials and access-policy changes require separate
explicit authorization. Public/global distribution is never inferred from local
adapter availability.
