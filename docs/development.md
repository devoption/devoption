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

