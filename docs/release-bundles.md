# Local platform bundle candidates

The offline assembler is the first slice of
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
inputs, release metadata and extracted notes. Python files remain nonexecutable;
the existing installation instructions deliberately select installed ownership
and executable permissions. There are no migration assets because this preview
implements no platform migration.

The embedded and adjacent `manifest.json` have identical bytes. Schema version 1
records the full source commit, platform version/target, seven crate versions,
the locked Wasmtime/WASI/HTTP runtime versions,
lockfile/metadata identity, required external software, compatibility/recovery
statements, and the size, normalized mode and SHA-256 of every payload file.
The manifest does not hash itself; `SHA256SUMS` identifies the complete archive
including that manifest. No credentials, host configuration, installed state,
build paths or timestamps are included.

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
sets `publication_eligible: false`, `provenance: "not-attested"` and
`sbom: "not-generated"`. The source commit is an assembly claim, not proof that
arbitrary supplied binary bytes came from a locked build. Authenticated CI build
provenance, an SBOM, independent verification, complete packaged-binary acceptance,
immutable publication/retry reconciliation and channel gates remain under #261.
Do not use these local candidates as an installer or recommended release target.
The [release approval gate](releases.md) remains separate.
