# Cargo registry dependency contract

`devoption-registry` implements the pure dependency contract in
[#119](https://github.com/devoption/devoption/issues/119), following the approved
direction: dependency acquisition through DevOption's registry/proxy, followed by
separately approved isolated offline builds. The current app/library runners use
verified operator-prepared inputs. There is no hosted registry server, network
acquisition client, Cargo archive store, credential transport or package publication
service. The [Cargo adapter](cargo-inputs.md) and explicit
[hosted registry bindings](hosted-cargo-inputs.md) validate supplied metadata;
bindings do not grant access or enable the existing service's hosted-source path.
The separate [library source-staging API](library-source-staging.md) retains private
canonical source archives/provenance, without Cargo ingestion, serving or publication.
The [Cargo generator](cargo-package-generation.md) creates candidate archive/index
bytes from verified library plans, without registry mutation or publication.
The [private candidate store](cargo-package-storage.md) retains them. A separate
[owner-only publication/access policy](package-publication-policy.md) records
installation-local decisions and gates byte acquisition. The opt-in
[owner controller](registry-controller.md) exposes staged-candidate controls;
a complete Cargo HTTP registry and runner materialization remain separate.

The crate is independent of the build/server crates. `devoption-builds` retains
the complete request/materialization in plans, while production source checks
and snapshot capture use the empty hosted-binding policy. See the
[reusable-package assessment](reusable-packages.md) for the implemented library
workflow and remaining distribution work.

## Minimal handoff

1. `CargoDependencyRequest` records the selected logical DevOption registry identity,
   SHA-256 of both exact manifest and lockfile bytes, and a claimed locked registry-package
   set, plus all explicit hosted-registry bindings. The supplied-byte adapter checks its supported graph, rejects unsupported
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
authorization. The [single-crate seed provider](offline-fixture-build.md) verifies
and owns actual supplied archive bytes; its only consumer is a trusted test.
A public `CargoMaterialization` constructor creates
metadata, not proof of registry identity, checked bytes, availability or permission.
The seed provider defines its bundle as the exact raw archive, with index and
configuration remaining separate fixed test context. A general physical bundle
format is still unspecified. Its future producer and consumer must verify the same
immutable bytes and expected package checksums;
hashing a serialized Rust struct or trusting a mutable cache directory is insufficient.

The [locked-archive policy adapter](package-publication-policy.md#exact-locked-archive-acquisition)
owns a complete admitted source-lock archive set with current hosted-project
authorization, source/registry/checksum bindings and fresh-policy revalidation.
The separate [vendor-bundle producer](cargo-vendor-bundles.md) verifies supported
archive/manifest contents and owns actual canonical `DVVENDOR1` bytes. It implements
`CargoRegistryProvider` for its exact captured request, with a descriptor bound to
those bytes. This is distinct from the worker's existing physical directory-tree
digest. Atomic publication, physical-tree verification and runner integration are
still required; matching metadata alone is not execution authority.

## Source identity and bounds

`CargoOrigin::CratesIo` preserves the original crates.io package namespace while
requiring delivery through DevOption's proxy. `CargoOrigin::DevOption(RegistryId)`
identifies a hosted registry, whose public/private visibility and access rules are
separate policy. Explicit bindings preserve the manifest name and exact canonical
Git/sparse HTTPS index identity; changing any supplied binding changes request
equality even when source bytes and package coordinates are unchanged. Matching names/versions in different origins remain distinct.
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
hexadecimal characters. The supplied-byte adapter computes input hashes, and
the seed provider also verifies its one archive. Other metadata constructors do
not verify package archive or bundle bytes.
Bundle sizes are `u64` claims, including zero; they do not authorize disk allocation.
Metadata bounds are not execution/resource policy. Errors omit supplied values,
and metadata types have no `Debug` implementation that could expose private names.

## Next dependencies and remaining decisions

The separate [root source verifier](cargo-source-proof.md) binds the adapter's
input bytes through supplied Git objects to a selected commit ID. Independently
trusted revision selection, complete source/context verification and actual
dependency completeness remain required. [Build plans](build-plans.md) now retain
that proof and require an exact match with the materialization's complete request.
Registry materialization
also needs verified immutable storage and explicit access and upstream policy.
The runner consumes the checked descriptor and verified bundle;
it must never silently contact an upstream when data is missing.

Fetching package bytes grants no right to run their code. The current
[trusted execution profiles](execution-trust.md) implement approval, environment,
owned-directory, isolation and resource rules for operator-prepared inputs.
A cache hit, binding set or Cargo offline flag cannot replace those boundaries.
Hosted acquisition/materialization still needs explicit integration with those
rules and registry/project authorization. Private registry credentials must not
enter build plans or build environments. The local release-artifact work in
[#18](https://github.com/devoption/devoption/issues/18) remains separate from
package publication and registry hosting.
