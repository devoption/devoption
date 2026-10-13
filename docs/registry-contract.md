# Cargo registry dependency contract

`devoption-registry` implements the pure dependency contract in
[#119](https://github.com/devoption/devoption/issues/119), following the approved
direction: dependency acquisition through DevOption's registry/proxy, then a
separately approved isolated offline build. Neither stage can run yet. There is
no registry server, network client, production provider, bundle format, storage,
credential transport, approval issuer or executor. A bounded
[supplied-byte Cargo adapter](cargo-inputs.md) validates a narrow manifest/lockfile
subset without reading a checkout or proving committedness.

The crate uses pinned TOML parsing and SHA-256 dependencies. It is independent of
the build and server crates. `devoption-builds` includes its immutable materialization descriptor in
every candidate plan. The running service does not call either domain.

## Minimal handoff

1. `CargoDependencyRequest` records the selected logical DevOption registry identity,
   SHA-256 of both exact manifest and lockfile bytes, and a claimed locked registry-package
   set. The supplied-byte adapter checks its supported graph, rejects unsupported
   semantics and binds both hashes. Source-commit linkage and completeness against
   actual dependency manifests remain future verification requirements; version
   requirements are not resolved here.
2. `prepare_dependencies` invokes the supplied `CargoRegistryProvider` once.
   The provider returns `CargoMaterialization`, containing the request plus the
   claimed SHA-256 and byte size of an immutable dependency bundle. No filesystem
   path, network URL, executable or credential crosses this interface.
3. The handoff rejects a response whose complete request differs, including
   registry identity, manifest, lockfile, package coordinates, origins or expected archive
   checksums. Provider errors propagate without retries or a public-fetch fallback.
4. The build plan owns this descriptor. Changing any dependency, origin, checksum,
   manifest, lockfile, provider identity, bundle digest or size changes plan equality.
   No approval exists yet; equality must not substitute for later approval binding
   and fresh validation of actual inputs.

Provider implementations are responsible for their effects and need independent
authorization. Only in-memory test providers exist. A public constructor creates
metadata, not proof of registry identity, checked bytes, availability or permission.
The physical bundle format is deliberately unspecified. Its future producer and
consumer must verify the same immutable bytes and expected package checksums;
hashing a serialized Rust struct or trusting a mutable cache directory is insufficient.

## Source identity and bounds

`CargoOrigin::CratesIo` preserves the original crates.io package namespace while
requiring delivery through DevOption's proxy. `CargoOrigin::DevOption(RegistryId)`
identifies a hosted registry, whose public/private visibility and access rules are
separate policy. Matching names/versions in different origins remain distinct.
Hosted packages cannot replace a crates.io package by name. Cargo source
replacement requires byte-identical mirrored packages; private packages require
their own registry identity ([Cargo source replacement](https://doc.rust-lang.org/cargo/reference/source-replacement.html)).

The initial contract supports these Cargo sources only. Additional upstreams,
Git dependencies, publishing and other package formats are unimplemented.
Logical registry IDs use 1–64 lowercase ASCII bytes: a leading letter followed by
letters, digits, hyphens or underscores. Package names use the same bound and
characters but permit uppercase letters and preserve exact spelling. Duplicate
coordinates compare names case-insensitively; provider matching still requires the
exact requested spelling. Versions use exact
SemVer spelling, at most 128 bytes, with `u64` core components; prerelease and build
metadata are supported. Duplicate origin/name/version coordinates are rejected,
including versions differing only in build metadata, matching Cargo registry
version uniqueness ([registry index](https://doc.rust-lang.org/cargo/reference/registry-index.html#version-uniqueness)).

Requests hold at most 1024 packages and are sorted by original registry, name and
version identity. Empty sets are supported. SHA-256 inputs require 64 lowercase
hexadecimal characters. The supplied-byte adapter computes input hashes, but
package archive and bundle bytes remain unverified.
Bundle sizes are `u64` claims, including zero; they do not authorize disk allocation.
Metadata bounds are not execution/resource policy. Errors omit supplied values,
and metadata types have no `Debug` implementation that could expose private names.

## Next dependencies and remaining decisions

The separate [root source verifier](cargo-source-proof.md) binds the adapter's
input bytes through supplied Git objects to a selected commit ID. Independently
trusted revision selection, complete source/context verification, plan binding
and actual dependency completeness remain required. Registry materialization
also needs verified immutable storage and explicit access and upstream policy.
The runner consumes the checked descriptor and verified bundle;
it must never silently contact an upstream when data is missing.

Fetching untrusted package bytes grants no right to run their code. A future
build can execute dependency build scripts; a cache hit and Cargo offline flags
are not a trust or isolation boundary. Approval lifecycle, environment allowlist,
work-directory ownership, actual network/process/filesystem isolation and resource
caps remain undecided and unimplemented. Private registry credentials must not
enter build plans or build environments. The local release-artifact work in
[#18](https://github.com/devoption/devoption/issues/18) remains separate from
package publication and registry hosting.
