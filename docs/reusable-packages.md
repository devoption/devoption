# Reusable libraries and packages

The platform does not yet provide an end-to-end build, publication and
cross-project consumption workflow for reusable packages. This assessment under
[#383](https://github.com/devoption/devoption/issues/383) separates source support
from execution and distribution. It concerns libraries in separate repositories,
not extracting or publishing the platform's internal `devoption-ui` crate.

## Current support

| Capability | Implemented boundary |
| --- | --- |
| Separate project/repository | Register a repository and inspect its tracked source with the existing project/Code flow. |
| Ordinary Rust library source | Explicit conventional `rlib` manifests pass bounded Cargo input inspection, source preparation and complete immutable source capture. This does not compile the library. |
| Hosted app compilation | The opt-in Linux runner builds a standalone `cdylib` for `wasm32-wasip2`, after review and approval, using verified operator-prepared offline dependencies. |
| Reusing existing crates.io packages | Exact-pinned dependencies and a complete verified archive/vendor set are supported for the web-app profile. Acquisition is separate operator setup, not a build-time download. |
| Hosted registry identity | Explicit binding APIs admit distinct hosted origins and preserve them through source proofs/plans. Existing service callers still use an empty binding set. See [hosted Cargo inputs](hosted-cargo-inputs.md); identity metadata is not a registry service. |
| Library build/test/package jobs | The trusted-adapter `LibraryBuildV1` API compiles natively, runs inline library unit tests, compiles for WASIp2 and saves a verified-source archive. The project interface selects this profile from captured source and provides verified local archive/receipt downloads; see [library builds](library-builds.md). |
| Library source staging | A trusted-adapter API privately retains immutable source archives and exact registry/version/source provenance, with conflict and corruption refusal. It is not connected to the service; see [source staging](library-source-staging.md). |
| Cargo package generation | A trusted-adapter API generates deterministic `.crate` bytes and candidate index lines from verified library plans or exact successful retained library builds; a native offline two-consumer fixture uses those generated artifacts. No publication or serving occurs; see [Cargo generation](cargo-package-generation.md). |
| Cargo candidate storage | A trusted-adapter API privately retains exact generated archives, candidate index lines and source provenance, with immutable versions and explicit complete-write recovery. The opt-in controller can explicitly stage successful retained-build candidates after owner review; see [candidate storage](cargo-package-storage.md). |
| Local package policy | A trusted-adapter ledger records owner approval, exact consumer-project grants and yanks, gates index/archive bytes and explicitly recovers complete interrupted decisions after fresh owner review. An opt-in owner-only JSON controller exposes these controls; runner integration is pending. See [controller controls](registry-controller.md) and [publication policy](package-publication-policy.md). |
| Exact locked archive acquisition | A native trusted adapter acquires the complete admitted source-lock archive set, verifies every checksum and hosted transitive grant/index edge, and revalidates current policy before reuse. Public archive contents and vendor materialization are separate; see [locked archives](package-publication-policy.md#exact-locked-archive-acquisition). |
| Cross-project or global package distribution | Owner-reviewed successful-build candidate staging is available. Project-authorized complete per-package index entries are available. No complete Cargo transport, dependency materialization or automatic dependency-update workflow exists. Build source callers still reject path, Git and custom registry dependencies. |
| Other package ecosystems | No npm or general package build/publication pipeline is implemented. |

The implementation references are [Cargo admission](cargo-inputs.md),
[source preparation](build-preparation.md), [immutable snapshots](source-snapshots.md),
[web-app builds](web-app-builds.md) and the [registry contract](registry-contract.md).
Earlier #119/#125 established the registry contract and a trusted offline fixture;
#295/#301 generalized app identity and assets, and #310 added a narrow
default-features-disabled dependency form. Their closure does not establish
ordinary library execution or a hosted package service. The open-issue review
for this assessment found no existing issue delivering those remaining outcomes;
#383 and its follow-ups track them explicitly.

A library maintained and published outside DevOption can already be consumed by
a supported hosted app as a crates.io dependency, provided its graph fits Cargo
admission, its code supports the app target/resource policy, and the operator
prepares and verifies all offline archives/vendor inputs. This existing route
does not provide library build/test jobs, publish the package, or automate
updates. Publishing a new package still needs explicit authorization.

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
Cross-project package consumption remains outstanding.

## Remaining delivery path

1. [#384: Library build and package profile](https://github.com/devoption/devoption/issues/384).
   The runner API now implements bounded compilation/unit tests and a canonical
   source package with version, revision and digest. Disposable acceptance covers
   native/Wasm compilation, a separate consumer and failure/recovery behavior.
   Authenticated project review/approval and retained package/receipt downloads
   now use the same lifecycle. Registry ingestion remains the next separate step.
2. [#385: Verified cross-project distribution](https://github.com/devoption/devoption/issues/385).
   Private generated package/index storage and explicit hosted identity APIs are
   implemented as trusted adapters. An owner-only local publication/access/yank
   policy now gates artifact reads. Complete build-output ingestion, approved Cargo index aggregation and verified offline dependency
   materialization, and two independent consumers with version-update and
   older-version reproducibility checks. Installation-local availability and
   globally reachable distribution must be demonstrated separately.

The remaining distribution requirements do not describe available services. Registry credentials,
public package publication, production installation, external availability and
access-policy changes require their own explicit authority. No existing web-app
release artifact or DevOption product release workflow substitutes for this
package lifecycle.
