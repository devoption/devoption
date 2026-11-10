# Explicit hosted Cargo input bindings

The first [#385](https://github.com/devoption/devoption/issues/385) slice adds
namespace-aware Cargo input admission and source proofs. It does not add a
registry service or enable hosted packages in the running platform. Existing
`inspect`, `validate`, root proof, snapshot verification and registered capture
entry points use an empty hosted-binding set and continue to refuse hosted sources.
The existing [library build/download workflow](library-builds.md) remains the
implemented user-facing path.

## Binding and admission contract

`HostedRegistry::new` binds one logical `RegistryId` to one canonical Cargo index
identity. The manifest registry name equals that logical ID. Index values are
HTTPS Git index URLs or `sparse+https://.../` sparse index identities. Credentials,
queries/fragments, percent-encoded paths, noncanonical spellings, unsupported
schemes and the reserved crates.io name/endpoints are rejected. A sparse index
requires its trailing slash. Each index is bounded to 2048 bytes. The owned
`HostedRegistries` set holds at most 16 bindings, sorted by ID, with unique names
and exact index identities. No implicit Git/sparse aliasing is performed.

This follows Cargo's distinction between a manifest registry name and its
configured [registry index](https://doc.rust-lang.org/cargo/reference/registries.html).
Cargo stores the protocol-specific index identity in the lockfile;
[its index guidance](https://doc.rust-lang.org/cargo/reference/registry-index.html#sparse-protocol)
recommends choosing one canonical protocol. Bindings use Cargo's actual lock
spelling: Git indexes get `registry+https://...`; sparse indexes retain
`sparse+https://.../` directly. A binding is supplied metadata, not evidence that
a registry exists, belongs to an owner, or is authorized for a project.

`inspect_with_registries` and `validate_with_registries` share the existing
manifest/hash/lock/graph bounds. They additionally accept an exact hosted pin:

```toml
[dependencies]
shared-components = { version = "=0.1.0", registry = "team" }
```

The caller must separately supply a `team` binding. Literal
`default-features = false` may be added; other features, renames, paths, Git,
optional/dev/build dependencies and workspaces remain unsupported. Existing
crates.io string pins and default-features-disabled pins keep their original
namespace. Unknown registry names or lock sources fail without inference or
fallback. A direct edge must match both the exact pin and the declared origin.

Lock edges may use `name`, `name version`, or `name version (source)`. Unqualified
references are accepted only when unique across all admitted origins. Equal
names/versions in different registries require the full exact source qualifier;
unknown, missing or ambiguous entries, repeated edges, cycles and unreachable
entries fail. Matching private coordinates cannot substitute for crates.io
coordinates. Source-qualified references to known crates.io entries are also
accepted. No package fetch, archive extraction or dependency resolution occurs.

Every supplied binding, including unused bindings, is retained in the
`CargoDependencyRequest` and participates in equality and provider matching.
Changing a binding set invalidates reuse of an earlier materialization/review,
even if the manifest, lock bytes and locked package set remain identical.
Caller-selected bindings still do not grant network, registry or execution access.

## Source proofs and integration boundary

`verify_root_with_registries`, `source_snapshot::verify_with_registries` and
`source_snapshot::registered::capture_with_registries` propagate the same binding
set through exact Git object membership and complete committed-source capture.
The registered adapter still compares every captured file against the checkout
and checks its selected revision and association. Bindings never come from
repository-local Cargo configuration. Source-only hashing excludes external
registry policy; full snapshot/plan equality includes that policy via the request.
The approval regression verifies that a changed set cannot consume an old review.

The existing service/CLI, installed configuration generator and worker still use
the crates.io-only path. A future integration must authorize and persist the
selected registry/project bindings, verify immutable package/index/archive/vendor
bytes and dependency completeness, render the reviewed context, and generate the
corresponding fixed offline Cargo configuration. It must keep separate origins
separate: Cargo [source replacement](https://doc.rust-lang.org/cargo/reference/source-replacement.html)
assumes identical bytes for a mirrored source; it is not a way to inject private
packages into crates.io. Binding construction is neither acquisition nor sandboxing.

## Validation

Ordinary tests cover sparse/Git spelling, canonical URL/name/set bounds,
credential and reserved-source refusal, duplicate bindings, exact pins/default
features, equal coordinates across origins, qualified ambiguity, graph failures,
input hashes, full-request provider matching and registered capture/approval
invalidation. Parsing uses the same pinned `url` version already used by the
server; no new dependency version or workspace crate is introduced.

The explicit trusted Unix fixture runs:

```sh
cargo test -p devoption-registry --test hosted_offline --locked --offline -- --ignored --nocapture
```

It uses the installed Cargo/compiler and a system linker, GNU timeout on Linux
or the system Python timeout wrapper on macOS. It creates only disposable source,
Cargo homes, targets and local-registry seeds, with cleared child environments.
It copies the checked-in verified public `itoa` archive into two distinct test
origins and packages a synthetic helper that depends on the hosted copy. Two
independent consumers use public `itoa` plus the hosted helper, forcing Cargo to
generate qualified lock references for identical coordinates in both origins.
The admission adapter must accept those actual graphs and preserve both origins.

The fixture builds/runs both consumers, updates one to a new exact helper version,
rebuilds the retained older version in a fresh cache, and requires failure for
missing/corrupt hosted archives in further fresh caches. No package is published
to an installation or public registry. This is a Cargo/metadata proof, not a
platform-hosted consumer acceptance test, OS network-isolation proof, access-control
acceptance or evidence of globally reachable distribution.

`InspectedInputs::package_dependencies()` additionally exposes each registry node's
validated outgoing lock edges, preserving full origin/version/checksum identities
in canonical package order. This reports supplied lock claims; it does not verify
actual dependency manifests or archives and grants no access. The
[locked-archive policy adapter](package-publication-policy.md#exact-locked-archive-acquisition)
uses those edges to compare hosted index dependencies before returning an exact
currently authorized archive set.

#385 also has private [source staging](library-source-staging.md),
[Cargo generation/storage](cargo-package-storage.md), and authenticated
[staging/publication/index controls](registry-controller.md). Complete physical
vendor verification/materialization and service/runner integration with actual
platform consumers remain open. Current service source callers still use empty
hosted bindings; native APIs and disposable Cargo proofs do not enable automatic
acquisition or global distribution.
