# Supplied Cargo input validation

`devoption_registry::cargo_inputs::validate` implements
[#121](https://github.com/devoption/devoption/issues/121). It accepts untrusted
manifest and lockfile byte slices, an explicit DevOption registry identity, and
expected SHA-256 values for both inputs. It has no filesystem, Git, Cargo,
process, environment or network access and no runtime caller.

The adapter hashes the exact bytes with pinned `sha2` and parses them with pinned
`toml`. Parser errors become fixed diagnostics that omit input text. TOML's
default recursion limits remain enabled. Size limits apply before hashing or
parsing: nonempty manifest up to 64 KiB, lockfile up to 512 KiB, at most 1024
registry packages plus one root, and 4096 dependency edges. Graph traversal is
iterative. These are input-validation bounds, not execution/resource permissions.

## Deliberately supported subset

- One standalone package. The manifest permits only `[package]` and an optional
  `[dependencies]` table. Package keys are exactly `name`, `version`, `edition`
  and `build`; all are required. Edition is `2021` or `2024`, and `build = false`
  disables automatic root build-script discovery. Package names and versions use
  the [registry contract's bounds](registry-contract.md#source-identity-and-bounds).
- Direct dependencies are string-valued exact pins, such as `http = "=1.2.3"`.
  The version spelling must exactly match the corresponding lock entry, including
  any prerelease/build metadata. This is stricter than Cargo's general SemVer
  requirement matching; no requirements are resolved here.
- A version-4 lockfile with only `version` and `[[package]]` at the top level.
  Package entries permit only `name`, `version`, `source`, `checksum` and
  `dependencies`. Exactly one entry has no source: it must match the manifest's
  package name/version and must not have a checksum.
- Every other package must use Cargo's canonical crates.io lock source,
  `registry+https://github.com/rust-lang/crates.io-index`, and contain a complete
  lowercase SHA-256 archive checksum. This string identifies original provenance;
  it is never used as a network destination. Acquisition remains behind DevOption's
  registry interface.
- Lock references are either a unique exact package name or `name version`.
  Qualified references support multiple locked versions. Missing or ambiguous
  references, repeated edges, duplicate registry coordinates, cycles and entries
  unreachable from the root are rejected. Root edges must exactly match the
  manifest's direct pins. Dependency-free packages are supported.

Everything outside that subset is rejected rather than ignored: workspaces and
workspace inheritance, path/Git/private registry dependencies, dependency tables,
renames, optional dependencies, features, target conditions, dev/build dependencies,
patches/replacements, profiles, explicit targets, other manifest metadata/keys,
source-qualified lock references and other lockfile versions. Even benign metadata
such as a package description is currently outside this narrow adapter. It cannot
consume DevOption's own workspace manifest.

## Identity and limits of the proof

Expected hash mismatches are rejected. Success returns an immutable
`CargoDependencyRequest` containing hashes of **both exact inputs**, including
comments and whitespace, and the canonical registry-package set from the supplied
lock graph. Provider matching and build-plan equality include both hashes. Editing
either input requires a different request, even when parsed package data is unchanged.

Caller-selected bytes and expected hashes do **not** prove committedness. A caller
can supply a different internally consistent graph and matching hashes. Success
does not establish that the lockfile reproduces Cargo resolution, includes every
dependency required by real package manifests/default features, or that archive
checksums match any actual package bytes. No archives are available to this adapter.
Its purpose is bounded input identity and consistency validation, not an execution
or trust decision.

The [root source verifier](cargo-source-proof.md) can supply these hashes after
checking a bounded commit/tree/blob membership proof for a selected SHA-1 ID.
That proves neither independent revision selection nor a complete source snapshot
or execution context. A future integration must establish those external facts,
including workspace context and Cargo configuration.
It must also verify source/target selection and the actual registry package/bundle
bytes against the descriptor. The registry layer must establish dependency completeness
against those verified inputs before approval. Toolchain identity, permissions,
owned work directories, isolation, resource limits and approval lifecycle remain
separate unimplemented gates. No network acquisition or execution is enabled here.

Tests use only checked-in synthetic fixture bytes and generated in-memory variants;
they neither inspect registered repositories nor run Cargo/Git. The fixture's package
checksums are synthetic and do not claim downloaded archive integrity.
