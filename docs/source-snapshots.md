# Immutable source snapshots

`devoption_builds::source_snapshot::verify` verifies and owns the complete supported
file tree of a separately selected Git SHA-1 commit. Added under
[#175](https://github.com/devoption/devoption/issues/175) for the
[#17 build workflow](https://github.com/devoption/devoption/issues/17), this is a
concrete in-memory source payload for a future controlled materializer. This pure
verifier performs no repository reads or execution. The separate
[registered adapter](#capture-from-a-registered-repository) added under
[#177](https://github.com/devoption/devoption/issues/177) collects its inputs through
the existing inspection root. Neither extracts files, invokes Cargo, acquires
dependencies or grants approval. Builds and `check-source` still provide
[working-tree observations](build-preparation.md); they do not create this snapshot.

## Verified and retained data

The caller supplies a separately selected full commit ID, its uncompressed commit
payload, one logical registry identity, and all reachable tree/blob payloads.
The verifier computes each typed Git object ID with the existing collision-detecting
SHA-1 implementation. It verifies the commit's selected identity and root tree,
walks every entry, and requires every referenced object with its expected type.
Missing, duplicate and unreferenced objects fail. Shared blobs/subtrees are supplied
once and expanded at every path, with separate expansion bounds.

The [root Cargo verifier](cargo-source-proof.md) and [Cargo adapter](cargo-inputs.md)
validate the exact committed manifest/lockfile bytes. A snapshot requires nonempty
UTF-8 `src/main.rs` and supports the single default binary named after the verified
package. Package/version, binary name and target path are read-only getters.
Presence does not prove syntax, successful compilation, HTTP behavior or that
source references stay inside the tree. Fixtures deliberately use `compile_error!`
without invoking a compiler.

`SourceSnapshot::files()` exposes sorted owned `SourceFile` values: canonical
relative path, Git blob ID, SHA-256 and exact bytes. All are regular non-executable
files; binary assets and empty non-target files are supported. Private fields and
read-only accessors preserve the verified value after caller buffers or checkout
files change. The snapshot remains usable after fixture storage is removed.

The snapshot SHA-256 hashes `devoption-source-snapshot-v1` plus a NUL, followed by
the selected commit's lowercase ASCII ID, commit-payload SHA-256, root-tree-payload
SHA-256, and each sorted file's path and content. Every field after the prefix has
an unsigned 64-bit big-endian byte-length prefix. Digests use 32 raw bytes; paths
use ASCII. The included manifest and fixed path determine the target. Registry
selection stays separately bound through the Cargo request and full plan equality;
changing the registry does not change source identity. This is not an approval token.

Candidate [build plans](build-plans.md) require the complete snapshot. Validation
matches commit, full dependency request, package and binary; a target mismatch
returns `TargetMismatch`. Plan/job equality retains the files, bytes, digest and
Cargo proof. The earlier two-file proof alone cannot create a candidate plan.

## Supported tree and bounds

- At most 256 supplied tree/blob objects, 128 expanded files and 256 expanded tree
  entries. Commit bytes are separate. Total input payloads including the commit
  and expanded file bytes each have a 4 MiB cap.
- Commit and each tree: 64 KiB. Each blob: 512 KiB. Cargo retains its narrower
  64 KiB manifest bound. Collection sizes are checked before indexing/hashing
  tree/blob payloads.
- Paths: at most 16 components and 1024 ASCII bytes. Components use 1–128
  letters/digits/`._-`, with no trailing dot, `.` or `..`. Trees require Git's
  canonical ordering and unique names even ignoring ASCII case.
- Only `40000` directories and `100644` files. Empty trees, executable files,
  symlinks, gitlinks, malformed entries and unsupported commit headers fail.
  Existing root-proof commit-header and SHA-1 detection limits still apply.
- `.git`, `.cargo`, `.gitmodules`, `.gitattributes`, `rust-toolchain` and
  `rust-toolchain.toml` fail at every depth, ignoring ASCII case. Nested Cargo
  manifests/lockfiles are unsupported.
- Root `build.rs`, `tests`, `examples`, `benches`, `src/lib.rs` and `src/bin` fail,
  ignoring ASCII case. The manifest subset permits no explicit targets/workspace
  and requires `build = false`. This first shape supports one standalone binary.

Failures return fixed diagnostics without source content or paths and no partial
snapshot. Bounded memory is allocated; no durable snapshot or extracted file is
created. Tests cover real disposable Git objects, missing/mutated links, types,
duplicates/extras, shared subtrees, targets, configuration, modes and bounds.

## Next integration boundary

Membership is relative to the selected commit, not proof of repository ownership,
authorized revision selection, a fresh catalog binding or a clean working tree.
The registered adapter below checks a supplied catalog association, selected current
HEAD and matching checkout bytes. A future authenticated workflow must select those
inputs, load a fresh catalog and revalidate them before review/approval. No existing
HTTP route, CLI command or background job calls the adapter.

A materializer must create and verify a separate owned work directory from the
exact files without changing the user's checkout, and control surrounding Cargo
configuration, toolchain and environment. Safe in-memory paths alone provide no
extraction safety or isolation. Registry archive verification, explicit approval,
persistent jobs and bounded execution remain separate prerequisites. This change
selects no production paths or runtime permissions.

## Capture from a registered repository

`source_snapshot::registered::capture` requires an explicit catalog snapshot,
project/repository IDs, `PermittedRoot`, full canonical selected commit and registry
identity. It revalidates saved associations and checkout/root identities, requires
the selected current HEAD and clean Git status. Unborn HEAD, older revisions,
changed bindings and foreign associations fail.

The project domain collects the commit and reachable tree/blob objects with fixed
`git ls-tree -r -t -z --full-tree`, `git cat-file --batch-check` and
`git cat-file --batch` commands. Full IDs and one fixed root-tree expression enter
a private request file. Object types/sizes are preflighted; payload responses must
match those headers and delimiters exactly. No parent history or unrelated objects
are requested. Ordinary packed objects and supported linked worktrees are supported.

Git uses the existing private temporary metadata and controlled settings, with
cleared environment, no source/global configuration, hooks, replacement objects,
lazy fetch or allowed network protocols. Raw reads use no filters or checkout
transformations. Every command/status check has a three-second deadline; failed,
timed-out or over-limit children are killed/reaped. Listing/header output is capped
at 64 KiB; content output at 4 MiB plus 64 KiB framing. Preflight payload totals are
at most 4 MiB, with at most 256 tree/blob objects, 64 KiB commit/tree payloads and
512 KiB blobs. Listing traversal is capped at 256 entries. These bound captured
output and payloads, not Git's internal memory allocation.

After graph verification, every snapshot file is compared against bounded raw reads
through the retained root. Symlinks, hard links and unsafe ownership/permissions
cannot supply bytes. The expected set is capped at 128 files, 512 KiB each and
4 MiB total. Reads and a final pass check file identity, size, permissions and
change timestamps; selected HEAD, metadata, clean status and saved bindings are
checked again. A successful capture runs seven bounded Git commands including four
status checks, plus local filesystem reads. It is not an atomic operation or a hard
realtime deadline for filesystem I/O.

Raw comparison catches edits masked by `assume-unchanged`, line-ending normalization
or ignored executable-mode changes. It conservatively rejects checkout bytes that
differ from committed bytes even when Git reports clean. Ignored/untracked files
never enter the snapshot; reported untracked changes fail. No catalog, index or
source writes occur. Temporary metadata is removed; returned verified bytes remain
owned in memory.

External filesystem writers and the selected local Git installation remain trusted.
Checks detect observed changes but cannot freeze the checkout or prevent later
edits; approval must not treat them as an ongoing lease. Source bytes stay immutable
after capture. Fixed errors distinguish stale selected commits or uncommitted source
without printing paths, source content or OS error details. Disposable tests cover
packed/linked storage, binary/nested files, hidden-dirty source, revision/binding
changes, unsupported inputs, protocol bounds and unchanged catalog/index/source.
No test builds or executes repository code.
