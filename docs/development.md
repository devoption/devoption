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

Each job uses a fresh GitHub-hosted Ubuntu runner. Rust jobs fetch locked inputs
before checking offline; no build cache or shared compiled artifacts are used.
Parallel jobs reduce elapsed time but can use more total runner time because
compilation/setup is repeated. The complete workspace test suite remains together
so Cargo continues to discover new crates and tests automatically.
