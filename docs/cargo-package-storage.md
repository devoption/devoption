# Private Cargo candidate storage

`devoption_builds::library::package_store::PackageStore` retains generated Cargo
archives and their candidate index lines under
[#385](https://github.com/devoption/devoption/issues/385). This Unix trusted-adapter
API performs private staging only. The [owner-only controller](registry-controller.md) can explicitly open an
operator-selected store and explicitly stage successful retained-build candidates
after owner review; the installer and runner do not automatically populate or enable it.
The store itself neither publishes nor serves a package. Installation-local
staging does not establish availability to other projects or external machines.

## Exact version records

`stage` accepts only a `CargoPackage` constructed by the
[generator](cargo-package-generation.md), not an imported archive or arbitrary
index line. Each versioned binary record retains:

- The complete staged source archive and its project, repository, Git revision,
  source digest, registry binding, exact package name/version and archive digest.
- The generated `.crate` bytes and actual SHA-256, index-relative path and exact
  newline-terminated candidate index bytes.
- For candidates generated through `History::library_candidate`, the successful
  attempt ID and exact build receipt (at most 16 KiB), bound to source identity,
  source archive bytes, package/version and `LibraryBuildV1` policy.
- Length framing and an outer SHA-256 covering the entire record.

Decoding checks lengths before allocation, the source record's own integrity,
the Cargo archive checksum, and the exact index path and line regenerated from
the archive's normalized manifest using the same metadata routine as generation.
This binds every direct dependency field (including registry, requirement and
default features), ordering, name/version, empty feature metadata and
`yanked=false` byte for byte. The shared bounded gzip/tar reader inspects regular
files in memory, with a 4 MiB expanded limit; malformed or oversized archives are
refused. No disk extraction, network access or Cargo execution occurs. Checksums
detect corruption; they are not signatures or protection against the trusted OS owner. A source-plan binding
is not proof of an approved/successful build or authorization to publish. Optional
build evidence is revalidated against retained source provenance when decoding;
it is not independent proof against OS-owner tampering. Original plan-only
`DVCARGO1` records retain their exact encoding. Build-bound candidates use
`DVCARGO2`; old readers fail closed on this format, so upgrade readers before
staging it. Neither record kind can replace the other at an occupied version,
even when Cargo archive/index bytes match. A later successful attempt or changed
receipt also conflicts; exact retries of the original candidate remain idempotent.

The version slot uses the existing [source staging](library-source-staging.md)
coordinate identity: logical registry ID, case-folded package name with underscores
normalized to hyphens, and version without SemVer build metadata. The record
retains exact spelling and index URL. Alias spellings and URL rebinding conflict
rather than creating replacement versions. Distinct registry IDs and release or
prerelease versions have separate slots.

`stage` returns `Stored` for a new version and `AlreadyPresent` for an identical
retry. Any changed archive, index, source, provenance or binding at an occupied
slot returns `Conflict` without replacement. `read` requires exact retained
coordinates and returns `None` only for absence. Corruption, unsafe storage and
binding mismatches return errors without upstream fallback. There is no deletion,
replacement, yank or publication operation. In particular, retained `yanked=false`
is candidate metadata, not a mutable published registry index.

## Storage and bounds

Use an existing absolute directory with no symlink components, owned by the
current non-root OS user with mode `0700`. Keep this directory separate from
source staging, checkouts, build scratch and untrusted processes. Each API uses a
different lock/pending name and record suffix; mixed inventories are refused.
Opening an invalid directory can leave its newly created private lock file; it
does not import or repair the contents. The operator owns directory selection.

The private descriptor-anchored storage engine is shared with source staging.
It holds an exclusive nonblocking advisory lock, rechecks the lock inode and
private directory/file identity, and validates the whole bounded inventory on
open, read and stage. Never delete or replace its persistent lock. Files must be
owned regular `0600` files with one hard link; symlinks, special files, unexpected
names, relocated keys and oversized/corrupt records are refused. Renaming the
store directory does not redirect an already opened handle to a replacement path.
All cooperating accessors must use the API; the OS owner and local filesystem
are trusted. No network-filesystem locking/durability guarantee is claimed.

A store retains at most 128 records. Each record is bounded by two 4 MiB archives,
a 256 KiB index line, 8192 bytes of source framing/provenance allowance,
and at most 16 KiB plus 20 framing bytes for successful-build evidence. The source
archive has its own tighter existing bounds. Inventory validation decodes one
record at a time. Capacity refusal retains existing versions and still allows
an exact retry; there is no automatic retention eviction. Operators must size
storage/backups for the full retained inventory and pending record.

Writes exclusively create a private pending file, sync and revalidate its bytes,
link it atomically into the final slot without replacing any destination, sync
the directory, unlink the pending name and sync the directory again. A write
failure poisons the handle and preserves remaining evidence. Ordinary open/read
refuses a pending write; it never assumes a partially written candidate exists.
The source-only record format and source-store refusal behavior remain unchanged.

## Explicit interrupted-write recovery

An operator/trusted adapter can call `PackageStore::recover(path, &expected)` only
after selecting the exact complete `CargoPackage` intended for that directory.
Recreate it from the same verified source and registry binding (and the same
retained attempt and exact receipt for build-bound candidates), or retain that
value outside the interrupted process. This is an explicit private storage
completion decision, not publication approval. No automatic startup recovery is
performed and no credentials are needed or created.

Recovery acquires the same exclusive lock and checks the entire inventory. It
requires the pending record to decode exactly equal to the supplied candidate:

- If only the complete pending file exists, sync it and complete the same
  no-replacement link/directory-sync/unlink/directory-sync sequence.
- If pending and final names already refer to the same inode with exactly two
  links, validate both and sync before removing only the pending name.
- If the pending name is absent and the exact final record already exists, return
  `AlreadyPresent`, allowing retry after interruption of recovery itself.

A partial/corrupt pending record, mismatched candidate, separate final inode,
wrong/extra hard link, unsafe inventory, occupied different version or missing
pending-and-final evidence is refused. Evidence remains for investigation. There
is no generic discard/repair/reset operation. Recovery cannot reconstruct missing
source or resolve a conflict by overwriting a version. After a failed recovery,
inspect actual storage rather than treating the error as proof that no durable
write occurred.

## Validation and remaining lifecycle

Workspace tests cover exact persistence/reopening, immutable retries, different
versions/registries, alias/rebinding conflicts, bounded decoding and cross-field
checks, capacity, unsafe inventory and separate source/Cargo record domains.
The existing source-store safety and failure-injection tests run against the
shared engine. Recovery tests cover complete pre-link and post-link evidence,
partial/conflicting/unsafe refusal and locking. A child-process fixture exits
abruptly without Rust writer cleanup at both commit boundaries; the parent
reopens, refuses ordinary use and explicitly recovers the exact package. These
are process/fault-injection checks, not a physical power-loss or filesystem test.

The explicit Cargo acceptance now stages and reloads generated base/helper
archives and indexes before seeding its disposable offline registry:

```sh
cargo test -p devoption-builds --lib generated_cargo_packages --locked --offline -- --ignored --nocapture
```

It builds two independent native consumers, updates an exact version, rebuilds an
older lock in a fresh cache and refuses missing/corrupt dependencies. This proves
compatibility of retained bytes, not authorization or consumption through the
running platform. No package is published to an installation or external registry.

The separate [local publication policy](package-publication-policy.md) now
records explicit owner approval, project readers and yanks, and checks those
decisions before returning retained index/archive bytes. It does not make staged
candidates automatically available.

Remaining #385 work includes successful-build candidate ingestion, complete
Cargo registry serving/index aggregation, complete verified dependency/vendor
materialization and service/runner integration. Native policy recovery is
documented in the [publication policy](package-publication-policy.md#fresh-review-of-an-interrupted-decision). Keep #385 and parent #383 open until those outcomes
are implemented and demonstrated. Global distribution, real credentials and
production installation/publication require separate explicit authorization.
