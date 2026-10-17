# Supplied Cargo input validation

`devoption_registry::cargo_inputs::validate` implements
[#121](https://github.com/devoption/devoption/issues/121). It accepts untrusted
manifest and lockfile byte slices, an explicit DevOption registry identity, and
expected SHA-256 values for both inputs. It has no filesystem, Git, Cargo,
process, environment or network access. The additional `inspect` entry point
shares the same parsing, hash computation, bounds and graph checks without an
expected-hash claim or registry selection. It returns package name/version, input
hashes, source-target shape and dependency count to the [source preparation flow](build-preparation.md).
`validate` retains its expected-hash check and explicit registry identity; neither
entry point proves source provenance or archive availability.

The adapter hashes the exact bytes with pinned `sha2` and parses them with pinned
`toml`. Parser errors become fixed diagnostics that omit input text. TOML's
default recursion limits remain enabled. Size limits apply before hashing or
parsing: nonempty manifest up to 64 KiB, lockfile up to 512 KiB, at most 1024
registry packages plus one root, and 4096 dependency edges. Graph traversal is
iterative. These are input-validation bounds, not execution/resource permissions.

## Deliberately supported subset

- One standalone package. The manifest permits `[package]`, optional `[dependencies]`, and the narrow
  `[lib]` form below. Package keys are exactly `name`, `version`, `edition`
  and `build`; all are required. Edition is `2021` or `2024`, and `build = false`
  disables automatic root build-script discovery. Package names and versions use
  the [registry contract's bounds](registry-contract.md#source-identity-and-bounds).
- An optional `[lib]` must contain exactly `name` and `crate-type = ["cdylib"]`.
  Its name must equal the package name with hyphens replaced by underscores and
  use ASCII Rust identifier syntax. Its conventional source is `src/lib.rs`;
  without `[lib]`, the source shape remains the default binary at `src/main.rs`.
  This admits the [selected web-app manifest](decisions/0004-first-wasm-web-app.md)
  under [#231](https://github.com/devoption/devoption/issues/231), but does not
  prove a Wasm target, HTTP ABI, source presence or a successful compilation.
- Direct dependencies are string-valued exact pins, such as `http = "=1.2.3"`.
  Core and prerelease spelling must match the corresponding locked version.
  Build metadata after `+` is ignored only for requirement matching, following
  [Cargo's version metadata rules](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#version-metadata).
  Thus `wasip2 = "=1.0.1"` matches locked `1.0.1+wasi-0.2.4`. The complete locked
  version and archive checksum remain in the request identity; changing metadata
  still changes that identity. Ranges and floating requirements remain unsupported.
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
patches/replacements, profiles, other explicit targets, other manifest metadata/keys,
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
