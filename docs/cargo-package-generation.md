# Cargo package generation

`devoption_builds::library::cargo_package::CargoPackage::from_plan` generates
bounded Cargo `.crate` bytes and one candidate index line from a structurally
validated library plan and explicit hosted-registry destination. This
[#385](https://github.com/devoption/devoption/issues/385) adapter performs no
filesystem, network, Cargo invocation, package publication or execution. The
service, CLI, workers and source store do not yet persist or serve these artifacts.

## Source, manifest and archive

Generation requires the existing conventional `rlib` source shape and exact
dependency subset. The adapter retains a [staged source value](library-source-staging.md)
with project/repository, registry binding, exact version, revision, source hash
and canonical source-archive provenance. This is a source-plan binding, not an
attestation of source authorization or build approval/success.

All captured source files remain in the package under `name-version/`, including
the committed lockfile. The original manifest bytes are saved as `Cargo.toml.orig`.
`Cargo.toml` is replaced with a deterministic normalized manifest containing the
same package name/version/edition, explicit conventional library target,
`build=false`, disabled automatic binary/example/test/benchmark discovery, and
exact direct dependency requirements/default-feature settings. Hosted registry
aliases become their exact reviewed `registry-index` URL. Public dependencies
receive Cargo's canonical crates.io index URL. Registry aliases are never resolved
from repository configuration or the process environment.

Captured paths whose first component case-insensitively matches `Cargo.toml.orig`,
`.cargo_vcs_info.json` or `.cargo-checksum.json` are rejected. The generator does
not overwrite user-supplied packaging/checksum/provenance metadata or construct
an archive with colliding generated paths. Source selection still requires review:
there are no package ignore rules and no automatic secret scanning/removal.

The normalized files are sorted by path and use the existing bounded canonical
ustar encoder, preserving its deterministic ownership, mode, timestamp, padding
and checksum fields. The original local `source.tar` format/bytes remain unchanged.
Both the expanded tar and compressed `.crate` are limited to 4 MiB; generated
manifest/original-manifest overhead counts toward the tar limit.

Compression uses pinned `flate2` 1.1.11 with its pure Rust backend, level 6, zero
mtime, OS byte 255 and no optional gzip filename/comment fields. This uses the
library's [gzip builder](https://docs.rs/flate2/latest/flate2/struct.GzBuilder.html)
without spawning a system compressor. SHA-256 is computed from the completed
`.crate` bytes, independently of the source archive's checksum. Identical source
and bindings produce identical archive/index bytes with this implementation and
lockfile. Future compressor/format changes must preserve already retained version
bytes; regenerating metadata is not permission to replace a version.

## Candidate index metadata

The adapter returns an index-relative path and a newline-terminated JSON line
following [Cargo's index format](https://doc.rust-lang.org/cargo/reference/registry-index.html).
The lowercase name determines the one/two/three/four-or-more-character path layout;
the original package spelling remains in metadata. The line includes exact
name/version, actual archive checksum, sorted direct normal dependencies with
requirements/default-feature flags, empty features and `yanked=false`. It is
bounded to 256 KiB. It contains no credential or download endpoint configuration.

Dependency registry fields preserve original namespaces: a dependency in the
destination registry gets JSON `null`, public crates.io dependencies get the
canonical public index URL, and dependencies in a different explicit hosted
registry retain that registry's canonical index URL. A manifest's alias is not
copied into the index as a registry identity. Public dependencies cannot become
hosted dependencies merely because their package names match.

This is metadata for one candidate version, not an index mutation, availability
announcement or publication approval. In particular, `yanked=false` does not
establish a lifecycle or recovery policy. The existing source store retains
original source archives only; it does not store `.crate`/index bytes or automatically
promote them. Imported registry archives and arbitrary caller-supplied index lines
are outside this generator.

## Validation and remaining work

Workspace tests verify deterministic archives/metadata, exact original-manifest
preservation, normalized dependency identities/defaults, destination-relative
registry fields, all index path lengths, reserved-path refusal and non-library
refusal. A source archive at the existing size limit is refused when generated
manifest/original-manifest overhead would exceed that limit. The explicit trusted Unix fixture runs:

```sh
cargo test -p devoption-builds --lib generated_cargo_packages --locked --offline -- --ignored --nocapture
```

It generates a hosted base library and two exact helper versions with the real
adapter, then supplies their bytes/index lines to a disposable local-registry
seed. The helper depends on the hosted base and the checked-in public `itoa`
fixture. Two independent consumers build and run with the installed Cargo/compiler
and separate public/hosted sources. It checks an exact-version update, an older
locked-version rebuild in a fresh cache, actual generated lock admission and
fresh-cache missing/corrupt dependency refusal without a final consumer binary.
Children use cleared environments, fresh Cargo homes/targets and fixed timeouts.
No package is published to an installation or external registry. This is native
Cargo compatibility evidence, not a platform-hosted/WASIp2 consumer acceptance,
OS network-isolation proof or project-authorization test.

Remaining #385 work includes durable Cargo archive/index ingestion, explicit
publication review/approval, visibility/read/write authorization, version/yank
and recovery semantics, complete verified dependency/vendor materialization,
service/runner integration and actual platform consumers. Installation-local and
globally reachable distribution still need separate implementation and evidence.
