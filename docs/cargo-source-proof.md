# Root Cargo source membership

`devoption_builds::cargo_source::verify_root` implements the bounded proof from
[#123](https://github.com/devoption/devoption/issues/123). It verifies supplied
Git object payloads and invokes the [Cargo input adapter](cargo-inputs.md).
It performs no repository reads, Git/Cargo invocation, network access, extraction
or build execution. It has no runtime caller and is not an approval gate.

## Concrete proof

Inputs are a separately selected full lowercase, nonzero SHA-1 commit ID, a
logical registry identity, and four untrusted, uncompressed payloads: commit,
root tree, root `Cargo.toml` blob and root `Cargo.lock` blob.

1. Compute the commit's Git object ID and require the selected ID.
2. Read its single root tree reference, then compute and match that tree's ID.
3. Parse the entire root tree, require canonical unambiguous entries, and locate
   both exact Cargo names with regular non-executable file mode `100644`.
4. Compute each typed blob ID and require its tree reference.
5. Supply the now-linked exact bytes and their computed SHA-256 values to the
   existing strict Cargo adapter. Unsupported Cargo semantics still fail.

The immutable `RootCargoSource` has private fields and records the selected
commit, SHA-256 of the exact commit/tree payloads, and the Cargo request with
both content hashes. The public getters are read-only. Under
[#167](https://github.com/devoption/devoption/issues/167), `BuildPlan` requires and
retains this result, matching **both** the selected commit and the complete
materialization request. Full plan equality includes the proof's commit/tree
SHA-256 identities. Under [#175](https://github.com/devoption/devoption/issues/175),
plans require the [complete source snapshot](source-snapshots.md) containing this
proof, all supported source bytes and the verified binary target. The two-file
proof alone is no longer sufficient. Plans still grant no approval or execution.

Git identity hashes `type + space + decimal byte length + NUL + payload`;
it is not a hash of file contents alone. The existing SHA-256 content values
therefore cannot substitute for Git object IDs. This follows the
[Git object format](https://git-scm.com/book/en/v2/Git-Internals-Git-Objects).
The pinned `sha1collisiondetection` 0.3.4 dependency uses collision detection;
the verifier rejects its collision error instead of accepting a replacement hash.
Detection mitigates known SHA-1 attacks, not all possible future collisions.
SHA-256 payload hashes retain exact proof identity after validation; they do not
retroactively strengthen the SHA-1 links. See the
[implementation's detection limits](https://docs.rs/sha1collisiondetection/0.3.4/sha1collisiondetection/).

## Supported shape and bounds

- Commit and root tree: nonempty, at most 64 KiB each. Commit headers must have
  one initial `tree`, zero to two distinct `parent` IDs, then nonempty single-line
  printable ASCII `author` and `committer` values, then a blank line. Metadata is
  opaque, not parsed or authenticated as a person/date. Message bytes are opaque;
  NUL is rejected throughout. Extra headers, signatures, encodings and octopus
  merges are unsupported. No parents or history are traversed.
- At most 1024 root entries. Names are 1–128 ASCII letters/digits/`._-`, without
  a trailing dot. Names must be unique even ignoring ASCII case, and use Git's
  ordering with a directory treated as ending in `/`. Empty/dot names, separators,
  non-ASCII names, truncation, trailing junk and zero IDs fail closed.
- Root entries may declare `100644`, `100755` or `40000` only. Symlinks, gitlinks,
  alternate mode spellings and executable/directory Cargo files are rejected.
  Other root blobs/subtrees are only references: their contents/types/existence
  have **not** been verified. This is not a complete source snapshot.
- The root rejects `.git`, `.cargo`, `.gitmodules`, `.gitattributes`, `rust-toolchain`
  and `rust-toolchain.toml`, ignoring ASCII case. This proves their absence only
  in the inspected root, without loading configuration. Cargo files must be at
  that root; nested packages and workspaces are unsupported.
- Manifest and lockfile retain the adapter's 64 KiB and 512 KiB limits. All four
  size checks happen before hashing or parsing. Errors omit supplied contents.
  No packs, deltas, compression, tags, refs, abbreviated IDs or Git SHA-256 format.

## Threat boundary and remaining proof

An attacker controls all four payloads. Changing a payload must match its typed
parent link and finally the selected commit, subject to SHA-1's stated limits.
But if the attacker also chooses the selected ID, an entirely different coherent
graph passes. Membership is relative to that ID; it proves neither repository
ownership, authorized revision selection, ref freshness, signatures nor existence
in a real repository. Pure bytes cannot establish those external facts.

Candidate plans now require a [complete source snapshot](source-snapshots.md).
Its separate registered adapter can collect and verify the selected current HEAD
and compare exact checkout bytes. A future authenticated workflow must select the
repository/revision and a fresh catalog through a trusted boundary, then establish
a controlled root working directory. Parent-directory/home/system Cargo configuration, environment,
toolchain selection, nested attributes/configuration, checkout transformations and
filesystem aliases are not inspected or controlled here. No extraction safety is
implied. Actual dependency completeness and registry archive/bundle verification
remain separate prerequisites before approval and execution.

Tests use synthetic object fixtures, independently pinned hash vectors, mutations
and a public synthetic collision vector with its license retained. They never
read a real repository or invoke Git.
