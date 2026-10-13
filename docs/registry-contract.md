# Cargo registry dependency contract

`devoption-registry` implements the pure dependency contract in
[#119](https://github.com/devoption/devoption/issues/119), following the approved
direction: dependency acquisition through DevOption's registry/proxy, then a
separately approved isolated offline build. Neither stage can run yet. There is
no registry server, network client, production provider, lockfile parser, bundle
format, storage, credential transport, approval issuer or executor.

The crate has no external dependencies. It is independent of the build and server
crates. `devoption-builds` includes its immutable materialization descriptor in
every candidate plan. The running service does not call either domain.

## Minimal handoff

1. A future trusted lockfile adapter supplies a `CargoDependencyRequest`: selected
   logical DevOption registry identity, SHA-256 of the exact lockfile bytes, and
   the complete locked registry-package set, including transitive/build dependencies.
   Version requirements are not resolved here. Source-commit and lockfile linkage,
   package-set completeness, path dependencies and unsupported source rejection
   remain responsibilities of that adapter.
2. `prepare_dependencies` invokes the supplied `CargoRegistryProvider` once.
   The provider returns `CargoMaterialization`, containing the request plus the
   claimed SHA-256 and byte size of an immutable dependency bundle. No filesystem
   path, network URL, executable or credential crosses this interface.
3. The handoff rejects a response whose complete request differs, including
   registry identity, lockfile, package coordinates, origins or expected archive
   checksums. Provider errors propagate without retries or a public-fetch fallback.
4. The build plan owns this descriptor. Changing any dependency, origin, checksum,
   lockfile, provider identity, bundle digest or size changes plan equality.
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
Logical registry IDs and package names use 1–64 lowercase ASCII bytes: a leading
letter followed by letters, digits, hyphens or underscores. Versions use exact
SemVer spelling, at most 128 bytes, with `u64` core components; prerelease and build
metadata are supported. Duplicate origin/name/version coordinates are rejected,
including versions differing only in build metadata, matching Cargo registry
version uniqueness ([registry index](https://doc.rust-lang.org/cargo/reference/registry-index.html#version-uniqueness)).

Requests hold at most 1024 packages and are sorted by original registry, name and
version identity. Empty sets are supported. SHA-256 inputs require 64 lowercase
hexadecimal characters; the crate neither computes hashes nor verifies content.
Bundle sizes are `u64` claims, including zero; they do not authorize disk allocation.
Metadata bounds are not execution/resource policy. Errors omit supplied values,
and metadata types have no `Debug` implementation that could expose private names.

## Next dependencies and remaining decisions

The next independent slice can validate committed Cargo manifests/lockfiles into
this request using fixtures, before selecting any live registry transport. Then
registry materialization needs verified immutable storage and explicit access and
upstream policy. The runner consumes the checked descriptor and verified bundle;
it must never silently contact an upstream when data is missing.

Fetching untrusted package bytes grants no right to run their code. A future
build can execute dependency build scripts; a cache hit and Cargo offline flags
are not a trust or isolation boundary. Approval lifecycle, environment allowlist,
work-directory ownership, actual network/process/filesystem isolation and resource
caps remain undecided and unimplemented. Private registry credentials must not
enter build plans or build environments. The local release-artifact work in
[#18](https://github.com/devoption/devoption/issues/18) remains separate from
package publication and registry hosting.
