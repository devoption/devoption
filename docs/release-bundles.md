# Local platform bundle candidates

The offline assembler is part of
[#261](https://github.com/devoption/devoption/issues/261). It packages a prepared
numbered preview from a clean committed checkout on Ubuntu 24.04 amd64. It does
not build, install, upload or publish anything. Its input executable must be a
trusted local build: assembly runs the snapshotted bytes with `--version` and
`--help`. Never pass a downloaded or untrusted executable to this tool.

## Assemble and inspect

Choose the embedded version and merge its metadata/notes before building. With
the pinned Rust toolchain and locked dependencies available:

```sh
cargo build --release --locked --offline
python3 scripts/release_bundle.py \
  --binary target/release/devoption \
  --output /tmp/devoption-bundle-candidate
cd /tmp/devoption-bundle-candidate
sha256sum --check SHA256SUMS
tar -tzf devoption-0.1.0-preview.1-ubuntu-24.04-amd64.tar.gz
```

Use the actual prepared version in the archive name. The output directory must
be new and outside the source checkout. A retry never overwrites existing bytes;
assemble into a second new directory to compare identical inputs. Tar headers,
ordering and gzip timestamps are normalized. This makes assembly repeatable for
identical inputs; it does not establish reproducible compilation. A crash can
leave incomplete output, which is rejected on retry and requires inspection.

The archive contains `bin/devoption` (server, CLI, UI and Wasmtime are compiled
into this executable), seven Python helpers, the three service templates, the
Caddy example, focused operator documents, license, locked dependency/toolchain
inputs, release metadata, extracted notes and `SBOM.cdx.json`. The vendored
Highlight.js browser build, source record and BSD license accompany its inventory
entry. Python files remain nonexecutable;
the existing installation instructions deliberately select installed ownership
and executable permissions. There are no migration assets because this preview
implements no platform migration.

The embedded and adjacent `manifest.json` have identical bytes. Schema version 2
records the full source commit, platform version/target, seven crate versions,
the locked Wasmtime/WASI/HTTP runtime versions,
lockfile/metadata identity, required external software, compatibility/recovery
statements, and the size, normalized mode and SHA-256 of every payload file.
The manifest does not hash itself; `SHA256SUMS` identifies the complete archive
including that manifest. No credentials, host configuration, installed state,
build paths or timestamps are included.

The adjacent SBOM is byte-identical to the archived copy. An adjacent unsigned
provenance statement binds the archive, manifest and SBOM digests; it remains
outside the archive to avoid a circular digest. The local checker also validates
both sidecars against regenerated values from the trusted inputs.

Ubuntu CI builds with locked offline inputs and assembles the actual release
executable. The assembler's version/help smoke checks run the exact bytes that
are archived. These checks supplement workspace and installation tests; they do
not establish a complete installation from the bundle or hosted-app acceptance.
`scripts/check_release_bundle.py OUTPUT --binary target/release/devoption`
independently compares the archive inventory, normalized metadata and complete
file bytes against the clean trusted checkout and local executable. It rejects
changed, missing, duplicate, linked and unexpected archive members without
extracting or executing them. This local-input comparison is not a verifier for
downloaded releases.

## Dependencies and trust limits

The supported assembly host is Ubuntu 24.04 amd64. Other distributions,
architectures and operating systems are rejected. Runtime operation still needs
the dedicated-node/systemd/cgroup-v2 profile and Python 3.11+, Git and sudo.
Acquire distribution packages through Ubuntu's signed package archives and record
the installed package inventory during host acceptance; this assembler does not
freeze an OS image or quantify minimum RAM/disk capacity.

The manifest carries the existing Caddy 2.11.7 Linux amd64 archive URL/SHA-512
pin from [public installation](public-linux-install.md). Assembly checks that
the documented pin agrees. App builds additionally need the source-pinned Rust
toolchain, `wasm32-wasip2` target and reviewed vendored inputs described in
[app builds](web-app-builds.md). None of these external dependencies are downloaded
or installed by the assembler. Future installer work must verify their acquisition.

SHA-256 beside an untrusted archive is not authentication. The manifest deliberately
sets `publication_eligible: false` and `provenance: "unsigned-assembly-statement"`.
The source commit is an assembly claim, not proof that arbitrary supplied binary
bytes came from a locked build. Authenticated CI build provenance, independent
verification, complete packaged-binary acceptance,
immutable publication/retry reconciliation and channel gates remain under #261.
Do not use these local candidates as an installer or recommended release target.
The [release approval gate](releases.md) remains separate.

## SBOM coverage

`SBOM.cdx.json` uses [CycloneDX 1.6](https://cyclonedx.org/specification/overview/).
It records every Cargo.lock package with a distinct package URL, version and
dependency edges, preserving parallel versions. Registry hashes identify crate
source archives, not compiled library code. Workspace packages reference the
source commit; the top-level application hash identifies the supplied executable.
It also inventories payload files and the embedded Highlight.js version/build hash
with its retained BSD-3-Clause license. Unknown or ambiguous dependency references,
unsupported package sources and missing registry checksums stop assembly.

This is a **source inventory**, including development and other-platform packages.
It does not assert which code the linker retained. Cargo.lock does not contain
third-party Cargo licenses, so these are not inferred. Rust standard libraries,
native system libraries, host OS packages, the separately installed proxy and
app-build toolchain are outside this SBOM's component graph. The CycloneDX
composition is explicitly `incomplete`; external requirements remain in the
manifest. This inventory is not a vulnerability or license-compliance assessment.

## Unsigned assembly statement

`provenance.unsigned.json` uses the
[in-toto/SLSA v1 statement structure](https://slsa.dev/spec/v1.2/provenance).
Its build type is this documented assembly operation: read a clean committed
checkout, inventory the fixed source files, snapshot a caller-supplied executable,
check its version/help, and normalize/package those bytes. `externalParameters`
records the target, platform version, caller-supplied binary origin and unsigned
authentication. `resolvedDependencies` records the source commit and executable,
lockfile, toolchain-file and release-metadata digests. Subjects identify the
archive, manifest and SBOM. `builder.id` identifies the source-pinned assembly
script; it is not an authenticated GitHub workflow identity. No environment
variables or caller-supplied URLs are copied into the statement.

The statement describes **assembly**, not the executable's compilation, and makes
no SLSA-level claim. It has no signature or trust root. An attacker can forge both
it and an archive; the local comparison checker requires the trusted original
checkout/build input. A future approved trusted-main workflow must bind locked
compilation and packaged-binary tests to those exact subjects and authenticate
its workflow/source identity. Verification must check signatures, expected
repository/workflow, reviewed source commit and subject digests before installer
use. [GitHub attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations)
can provide that authenticated identity. Attestation writes, signing/credential
enablement and publication are not enabled by this implementation.
