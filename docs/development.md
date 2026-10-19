# Development

Follow the [contribution guide](../CONTRIBUTING.md) and install the
[quick-start prerequisites](../README.md#quick-start) first.

## Architecture

See the [architecture map](architecture.md) for crate responsibilities, implemented
data flow and planned boundaries. Focused references are in the
[documentation index](README.md).

## Checks

For checks, you also need Python **3.11+** and Node.js **22.22.3**. No Python or npm
packages are required. From the repository folder, run:

```sh
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 -m unittest discover -s scripts -p 'test_*.py'
node --test scripts/test_*.cjs
python3 scripts/release_candidates.py audit
```

Workspace tests include [trusted leaf subprocess fixtures](process-supervision-fixtures.md)
on Unix and require `/usr/bin/python3`; these exercise real TERM/KILL handling
without launching repository workloads.

On Linux, also run the explicit trusted fixture build (GNU `timeout` and the
installed Rust toolchain/linker are required):

```sh
cargo test -p devoption-registry --test offline_build --locked --offline -- --ignored --nocapture
```

## Continuous integration

Code validation runs when a PR is opened, reopened, or receives new commits, on
pushes to `main`, and through the reusable release check. Marking a draft PR ready
for review does not restart CI. PR title/body edits run only the separate
contribution workflow, which validates issue references and change documentation.

CI runs four independent jobs in parallel: Rust formatting/Clippy, workspace tests
and the debug build (including the explicit trusted offline fixture), the release
build and installation fixtures, and Python/Node checks. Each of these jobs checks for
tracked-file changes. The required `Rust and repository checks` status succeeds
only when all four jobs succeed; failure, cancellation, or skipping cannot pass
that gate. `Issue and change documentation` remains a separate required status.
This preserves branch protection without changing repository settings.

The installation job also assembles a local preview bundle from the release
executable, runs its version/help checks, and compares every archived byte and
manifest entry, source-inventory SBOM and unsigned assembly statement with the
trusted checkout/build inputs. It uploads no artifacts;
see [bundle candidates and remaining trust gates](release-bundles.md).

The Python job installs pinned release-plz and Rust to run the real-tool release
fixtures on Ubuntu, including ordinary PRs. Local runs without
`DEVOPTION_RELEASE_PLZ` still skip tool-dependent cases; use the pinned binary
when validating release preparation changes. Failed captured commands retain a
bounded diagnostic in their traceback.

Each job uses a fresh GitHub-hosted Ubuntu runner. Rust jobs fetch locked inputs
before checking offline; no build cache or shared compiled artifacts are used.
Parallel jobs reduce elapsed time but can use more total runner time because
compilation/setup is repeated. The complete workspace test suite remains together
so Cargo continues to discover new crates and tests automatically.

The Python/Node job fetches full Git history and audits committed
[candidate reservations](release-candidates.md), including their original source
versions. The required contribution check additionally rejects edits or deletions
of reservations already present in the PR base. Local append-only validation uses
`python3 scripts/release_candidates.py audit --base REVIEWED_40_HEX_BASE_COMMIT`.
