# Reusable libraries and packages

The opt-in installed Linux runner and private owner-controlled registry support
building a separate Rust library, explicitly staging/publishing an immutable
version, granting consumer-project reads and building independent WASIp2 apps
from their exact committed locks. This bounded workflow addresses
[#383](https://github.com/devoption/devoption/issues/383) and
[#385](https://github.com/devoption/devoption/issues/385).

## Current support

| Capability | Implemented boundary |
| --- | --- |
| Separate project/repository | Register a repository and inspect its tracked source with the existing project/Code flow. |
| Ordinary Rust library source | Explicit conventional `rlib` manifests pass bounded Cargo input inspection, source preparation and complete immutable source capture. This does not compile the library. |
| Hosted app compilation | The opt-in Linux runner builds a standalone `cdylib` for `wasm32-wasip2`, after review and approval, using verified operator-prepared offline dependencies. |
| Reusing existing crates.io packages | Exact-pinned dependencies and a complete verified archive/vendor set are supported for the web-app profile. Acquisition is separate operator setup, not a build-time download. |
| Hosted registry identity | Explicit binding APIs admit distinct hosted origins and preserve them through source proofs/plans. The controller uses only independently configured bindings. See [hosted Cargo inputs](hosted-cargo-inputs.md); identity metadata is not a registry service. |
| Library build/test/package jobs | The trusted-adapter `LibraryBuildV1` API compiles natively, runs inline library unit tests, compiles for WASIp2 and saves a verified-source archive. The project interface selects this profile from captured source and provides verified local archive/receipt downloads; see [library builds](library-builds.md). |
| Library source staging | A trusted-adapter API privately retains immutable source archives and exact registry/version/source provenance, with conflict and corruption refusal. It is not connected to the service; see [source staging](library-source-staging.md). |
| Cargo package generation | A trusted-adapter API generates deterministic `.crate` bytes and candidate index lines from verified library plans or exact successful retained library builds; a native offline two-consumer fixture uses those generated artifacts. Publication and serving require the separate policy/controller; see [Cargo generation](cargo-package-generation.md). |
| Cargo candidate storage | A trusted-adapter API privately retains exact generated archives, candidate index lines and source provenance, with immutable versions and explicit complete-write recovery. The opt-in controller can explicitly stage successful retained-build candidates after owner review; see [candidate storage](cargo-package-storage.md). |
| Local package policy | A trusted-adapter ledger records owner approval, exact consumer-project grants and yanks, gates index/archive bytes and explicitly recovers complete interrupted decisions after fresh owner review. An opt-in owner-only JSON controller exposes these controls; build preparation and approval recheck current grants. See [controller controls](registry-controller.md) and [publication policy](package-publication-policy.md). |
| Exact locked archive acquisition | A native trusted adapter acquires the complete admitted source-lock archive set, verifies every checksum and hosted transitive grant/index edge, and revalidates current policy before reuse. Public archive contents and vendor materialization are separate; see [locked archives](package-publication-policy.md#exact-locked-archive-acquisition). |
| Verified vendor bundles | A native adapter verifies supported archive/manifest content and lock edges, generates disjoint directory sources/checksums and owns a deterministic byte bundle. The installed worker mounts reverified attempt-local inputs; see [vendor bundles](cargo-vendor-bundles.md). |
| Cross-project or global package distribution | Owner-reviewed successful-build candidate staging is available. Project-authorized complete per-package index entries are available. The controller materializes approved exact hosted dependencies for offline worker builds. There is no public Cargo transport or automatic update/deployment; path and Git dependencies remain unsupported. |
| Other package ecosystems | No npm or general package build/publication pipeline is implemented. |

The implementation references are [Cargo admission](cargo-inputs.md),
[source preparation](build-preparation.md), [immutable snapshots](source-snapshots.md),
[web-app builds](web-app-builds.md) and the [registry contract](registry-contract.md).
The implemented local workflow is deliberately bounded. It does not expose an
anonymous/public Cargo registry, fetch missing upstream archives, automate
version updates or deploy consumer artifacts. Publication and every build retain
their separate owner approvals. Existing operator-prepared crates.io builds also
remain supported when registry support is disabled.

## Prepare a separate library

A standalone library can use the following deliberately narrow source shape:

```toml
[package]
name = "shared-dashboard"
version = "0.1.0"
edition = "2024"
build = false

[lib]
name = "shared_dashboard"
crate-type = ["rlib"]
```

Commit `src/lib.rs` and a version-4 `Cargo.lock`, including for a library. Generate
and check the lockfile with Cargo outside DevOption. The lockfile root must match
the package, with the complete locked registry graph for any dependencies.
The [Cargo input subset](cargo-inputs.md) remains authoritative: package metadata,
features, dev dependencies, workspaces and custom paths are currently outside
that subset. A conventional `cargo new --lib` manifest needs the explicit fields
above before these source checks can pass. Library name must match the package
name with hyphens changed to underscores. Absence of `[lib]` still selects the
existing binary source shape; source inspection does not infer targets from disk.

Keep reusable rendering code independent of a server/runtime where practical.
For reuse by hosted apps, verify that dependencies support `wasm32-wasip2`.
CSS/JavaScript assets can be committed and embedded by the library/consumer using
the existing [app asset contract](web-app-builds.md#application-identity-and-assets).
A reusable library is linked into each consumer's app component; each consumer
needs its own build and deployment to receive an update. An `rlib` is a
compiler-specific output, not the portable source package to distribute between
projects. No separate library runtime route is required.

Source admission is implemented, but passing source checks does not establish
buildability or dependency availability. A
separate library can be developed and tested with ordinary Cargo immediately;
the [library runner acceptance](library-builds.md#validation) exercises that shape
on a disposable Linux node. A real supported library can use that project build/download flow once installed.
Cross-project consumers use the explicit registry-root workflow below.

## Build, publish and consume

1. Register the library checkout, commit its supported source/lockfile and run the
   [library build/test/package workflow](library-builds.md) through owner approval.
2. With the explicit private registry root enabled, [review and stage the exact
   successful build](registry-controller.md#successful-build-candidate-staging).
3. Separately approve publication of its immutable version and grant the intended
   consumer project IDs read access through the [controller controls](registry-controller.md).
4. Register each consumer checkout in its own project. Commit its exact hosted
   manifest dependency and complete v4 lockfile. Use the existing build review
   and approval flow; it checks current grants and exact archives, stages verified
   [attempt-local vendor inputs](cargo-vendor-bundles.md), and runs locked/offline
   inside the installed worker.
5. For an update, build/stage/publish a new version, commit the selected consumer's
   new manifest/lockfile and approve its new build. Other consumers retain their
   old locks and remain rebuildable while those versions and reader grants exist.
   Each consumer deployment requires its separate existing approval.

The disposable installed-controller/worker acceptance covers one library and two
independent WASIp2 consumers, an approved update, retained older-lock rebuilding,
and missing/corrupt/revoked/tampered-input refusal. The separate native Cargo
proof also covers mixed public/hosted dependency closure and fresh yank resolution.

The supported manifest/archive profile is documented in the linked source and
vendor guides; broader feature/target/build-script semantics are refused. Registry
credentials, public package publication, production installation and externally
reachable hosting require separate authority and implementation. No product
release or web-app release artifact substitutes for this package lifecycle.
