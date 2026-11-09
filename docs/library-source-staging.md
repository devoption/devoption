# Immutable library source staging

The next [#385](https://github.com/devoption/devoption/issues/385) slice adds
`devoption_builds::library::staging`, a Unix trusted-adapter API for durable,
private source staging. It is separate from publication, package serving and
dependency materialization. The service, CLI, installer and workers do not open
this store yet. The existing [library build/download workflow](library-builds.md)
remains the user-facing path.

## Source and version identity

`StagedSource::from_plan` accepts a structurally validated `BuildPlan` and an
explicit [hosted registry binding](hosted-cargo-inputs.md). It requires an `rlib`
source snapshot and generates the existing deterministic `ustar-source-v1`
archive directly from the complete verified source bytes. It never reads build
output or accepts a caller-supplied archive. No Cargo subprocess or extraction
occurs. This is a source archive, not a registry `.crate` or a compiled `rlib`.

The immutable record retains the exact destination registry ID/index identity,
package name/version, project/repository IDs, Git revision, complete source
SHA-256, archive SHA-256 and archive size. `archive()` returns owned, rechecked
source bytes after a read. Private metadata/bytes have no `Debug` implementation;
fixed error messages omit coordinates, URLs, paths and source text.

These fields establish binding to the supplied source plan. They do not attest
that the repository/revision was authorized or that a build was approved,
executed or successful. Project/repository IDs are provenance, not access grants.
The caller remains responsible for separately selecting/authenticating the source
and registry destination. Staging grants no right to publish or execute it.

One version slot is derived from length-framed logical registry ID, case-folded
package name with underscores normalized to hyphens, and version excluding
SemVer build metadata. Alias spellings therefore cannot create a second source
at the same version. The retained record preserves the original exact spelling.
The index URL does not change the slot: rebinding an existing logical registry
ID to another URL conflicts instead of shadowing its versions. Different logical
registries and different release/prerelease versions have separate slots.

`stage` returns `Stored` for a new record or `AlreadyPresent` for an identical
retry. Any changed bytes, source revision/hash, provenance, binding or spelling
at an occupied slot returns `Conflict` without writing. `read` requires the exact
retained coordinates/binding. Only an absent slot returns `None`; aliases,
corruption and unsafe storage return errors, with no lookup/fetch fallback.

## Storage, bounds and interrupted writes

`SourceStore::open` requires an existing absolute directory with no symlink
components. The final directory must be owned by the current non-root OS user
with mode `0700`. It creates only a private persistent lock file, takes an
exclusive nonblocking advisory lock for the handle's lifetime, and validates
the complete bounded inventory before accepting the store. All cooperating
accessors must use this API. Keep the directory separate from checkouts and
untrusted processes; this API does not configure or verify that deployment layout.

The store is anchored to its directory descriptor, including if the directory
is renamed. It checks that the retained lock and its directory entry still
identify the same private file on each operation and before committing a write.
Never delete or replace the lock file. A second handle returns `Busy`.

There are at most 128 retained records. Each archive is bounded by the library
packager's 4 MiB limit; the complete versioned binary record is at most 4 MiB
plus 4096 bytes. Each string and length is checked before decoding/allocation.
A trailing SHA-256 covers all metadata and archive bytes, in addition to the
archive's own recorded digest/size. Every open/read/stage checks the inventory,
record keys, bounded contents and digests, using at most one decoded record at
a time during inventory validation. Records/lock must be regular, owned, private
`0600` files with one hard link; symlinks, special files, unexpected entries,
renamed records and oversized/corrupt data fail closed. Reads also check file
size and modification/change timestamps around the bounded read.

A new record is written to an exclusively created private pending file, synced
and rechecked, then atomically linked to its final version key without replacing
any existing destination. The directory is synced before removing the pending
link and synced again afterward. There is no record replacement or deletion API.
An identical retry after a completed write does not rewrite the file.

Any failure after pending-file creation poisons the handle and preserves remaining
evidence. A crash can leave a partial pending record, or a complete final record
with a pending hard link. Reopening refuses pending/unsafe state. Do not blindly
delete pending files or retry through another writer: explicit inspection/recovery
must establish the intended version and durability before reuse. Source-store recovery remains manual. The separate
[Cargo candidate store](cargo-package-storage.md) offers explicit exact-candidate
completion; that operation does not open or repair source-store records. The OS owner and local filesystem are trusted;
checksums are corruption checks, not signatures or protection against that owner.
No network filesystem durability/locking guarantee is claimed.

## Validation and remaining integration

Workspace tests cover exact source/provenance persistence after reopening,
idempotent retry, distinct versions/registries, alias/index/provenance conflicts,
capacity, malformed/truncated/tampered records, unsafe directory/file state,
symlink/hard-link refusal, renamed records, exclusive locking and replaced locks.
Injected partial/short writes and a late occupied destination exercise the same
write path, proving refusal and evidence preservation without overwriting the
destination. No workload or package publication is required by these tests.

This implements source-package staging and private metadata lookup. A separate
[Cargo generator](cargo-package-generation.md) produces normalized archive and
candidate index bytes from source plans; the separate [Cargo candidate
store](cargo-package-storage.md) retains those bytes. Source staging does not
implement Cargo archive ingestion or an index service, public/private
visibility grants, registry authentication/serving, publication approval,
yank/recovery policy, dependency closure verification, offline runner integration,
or consumers through the running platform. Those remain open under #385.
Installation-local staging establishes no globally reachable distribution.
