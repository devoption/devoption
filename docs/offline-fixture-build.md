# Verified seed to a real fixture build

[#125](https://github.com/devoption/devoption/issues/125) exercises actual dependency
bytes and a real Cargo compile. It is trusted repository test code, not a production
executor or an untrusted-code runner. Builds remain unavailable in the application.

## Provider and bundle

`devoption_registry::seeded::SeededCargoProvider` accepts an exact Cargo request and
one nonempty preseeded crates.io archive up to 1 MiB. It requires exactly one package,
hashes the actual supplied bytes against the lock entry's archive checksum, then
owns a private immutable copy. Missing, corrupt, oversized and unsupported seeds
fail before any bundle can be returned. A different complete request cannot reuse it.

For this narrow provider, **the bundle is exactly the raw `.crate` archive**. Its
materialization digest/size describe those bytes, not a serialized metadata struct.
`bundle()` goes through the existing checked provider handoff and returns read-only
bytes together with their matching descriptor. This adds no disk/network access,
archive parser, package resolver or general multi-package bundle format.

The caller still supplies the expected request/checksum. Matching it proves byte
integrity relative to that expectation, not authenticity or permission to execute.
The provider does not parse arbitrary archives or prove dependency completeness.
The index and source-replacement configuration are separate, fixed test inputs;
they are not covered by this single-archive bundle digest. Production consumption
must bind and verify that additional context before approving a build.

## Linux functional proof

The explicit CI step runs:

```sh
cargo test -p devoption-registry --test offline_build --locked --offline -- --ignored --nocapture
```

It requires the already installed Cargo/compiler used for the test, the system
linker and GNU `/usr/bin/timeout`. It downloads no toolchain or dependency. The
ordinary workspace test run intentionally leaves this Linux-only test ignored;
CI runs it once separately so the artifact evidence appears in its logs. It has
no tests on other operating systems.

The test validates the checked-in manifest/lockfile with the existing Cargo adapter,
obtains the verified archive from the provider, and writes it into an owned temporary
local-registry layout. Cargo consumes it through
[standard source replacement](https://doc.rust-lang.org/cargo/reference/source-replacement.html).
DevOption does not implement archive extraction. The seed is the real pinned public
`itoa` archive; see [fixture provenance and licensing](../crates/devoption-registry/tests/fixtures/README.md).

Each valid/missing/corrupt case gets a fresh temporary source directory, Cargo home,
home, scratch directory and output directory. Source files are read-only, and their
exact bytes are compared after every attempt. The child environment is cleared and
only fixed test settings are supplied. The command is fixed:

```text
cargo build --release --locked --offline --package offline-fixture --bin offline-fixture
```

There is one compile job, a 60-second test timeout with a five-second kill grace,
and no shared dependency/output cache between cases. A successful compile produces
one artifact under 4 MiB; the test hashes its actual bytes, invokes only that fixed
trusted fixture with a five-second timeout, checks `verified seed: 42`, and prints
the artifact SHA-256/size. The artifact hash is evidence for that run, not a claim
of reproducibility across hosts/toolchains. It is deleted with the temporary tree,
not deployed or published.

Unit tests reject missing/corrupt seeds at the provider boundary. The Linux proof
also omits or corrupts the on-disk archive after handoff. Fresh Cargo homes prevent
the earlier successful case from masking those failures; both must return Cargo's
failure status with no artifact. The configured source replacement and offline
flags provide no upstream fallback. This does **not** prove OS network isolation.

## Remaining production decision

The temporary directories, cleared environment and timeout are test controls, not
an approved production execution profile. No arbitrary user command, registered
repository, instance data, runtime route, owner approval or job record is consumed.
No custom Git proof is used as execution authority.

Before user-repository builds, choose and authorize one execution profile: OS account,
owned source/work/artifact paths, allowed toolchain/environment, filesystem/network
restrictions and resource limits. Then bind independently selected complete source,
actual dependency context and bytes to explicit owner approval, and implement
persisted single-job status, cancellation, cleanup and logs. This test authorizes
none of those permissions and performs no deployment.
