# DevOption

DevOption is an early open-source project working toward a self-hosted software
factory: a place to manage the work of building, testing, and running software.

**Today this repository is a small foundation, not a working software factory.**
It contains a Rust workspace, one command-line program with help and version
output, behavioral tests, and contribution and release checks. The program does
not start a server, create files, provision infrastructure, or contact a service.
There is no installer, product UI, runtime cluster, or published release yet.

## Try it locally

You need Git, Rust **1.99.0**, and your operating system's C build tools/linker.
Use the [official Rust installation guide](https://www.rust-lang.org/tools/install)
if Rust is missing; it explains platform prerequisites. With rustup installed,
the checked-in `rust-toolchain.toml` selects the version and components
automatically. An existing system Rust installation at the same version also works.
The first rustup setup needs internet access; this crate has no third-party Rust
dependencies and builds offline afterward.

In a terminal:

```sh
git clone https://github.com/devoption/devoption.git
cd devoption
cargo run --locked --offline -- --help
cargo run --locked --offline -- --version
```

Help describes the scaffold's limits. The version is `devoption 0.1.0-dev`;
this identifies development source, not a release. Running without arguments
also shows help. Unsupported commands (such as `start`) exit with code 2 and
an error on standard error.

## Build and check

Run these commands from the repository folder:

```sh
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```

The executable is `target/debug/devoption` (`devoption.exe` on Windows).
Tests exercise the actual command's help, version, and invalid-input behavior.
GitHub CI runs on Ubuntu; local validation also covers macOS. Windows is not yet
covered by CI.

Repository automation uses Python **3.11 or newer**, with no external Python
packages. To check it too:

```sh
python3 -m unittest discover -s scripts -p 'test_*.py'
```

## Where this is going

DevOption's own code will stay in **one monorepo**, split into Rust modules and
crates when real boundaries justify them. There is one crate now:
`crates/devoption-cli`. No services or placeholder crates are included.

Future work may add easy self-hosted installation, a full administration and
development factory cluster, lighter runtime clusters, source control, CI,
releases, and observability. Managed projects may span multiple repositories
and share reusable internal packages. Agent access through MCP is intended to
be bounded and secure. These are design goals, not available capabilities or
delivery promises.

## Get involved

Start with a small [issue](https://github.com/devoption/devoption/issues/new/choose)
before changing code or documentation. See [CONTRIBUTING.md](CONTRIBUTING.md),
[the changelog](CHANGELOG.md), [release instructions](docs/releases.md), and
[security reporting guidance](SECURITY.md).

DevOption is licensed under the [MIT License](LICENSE), copyright 2026 DevOption
Labs LLC. You can [sponsor DevOption on GitHub](https://github.com/sponsors/devoption).
