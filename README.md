# DevOption

DevOption is an open-source project working toward a self-hosted software factory:
a place to build, test, and run software.

It currently provides a local web preview with Home, Projects, and About
placeholders, plus browser-local Appearance settings. Project registration,
builds, and deployments are not implemented yet. There is no published release.

## Try it locally

You need Git, Rust **1.99.0**, and your operating system's C build tools/linker.
The [Rust installation guide](https://www.rust-lang.org/tools/install) covers setup;
`rustup` uses the version pinned in this repository. Initial setup requires internet
access to download the toolchain and dependencies.

```sh
git clone https://github.com/devoption/devoption.git
cd devoption
cargo fetch --locked
cargo run --locked --offline -- serve
```

Open [127.0.0.1:3000](http://127.0.0.1:3000/). Use the profile menu to open
Appearance settings. Press **Ctrl+C** in the terminal to stop the server.
Run `cargo run --locked --offline -- --help` for command options.

This local preview binds to `127.0.0.1` and accepts only `127.0.0.1` or `localhost`
at its listening port. It has no authentication or TLS; Host validation is not
CSRF protection. Do not expose it as a public service.

## Screenshots

**Home**

![Home page](docs/screenshots/home.jpg)

**Projects**

![Projects page](docs/screenshots/projects.jpg)

**About**

![About page](docs/screenshots/about.jpg)

**Appearance**

![Appearance settings](docs/screenshots/appearance.jpg)

## Contributing

Start with an [issue](https://github.com/devoption/devoption/issues/new/choose)
and read the [contribution guide](CONTRIBUTING.md).

For checks, you also need Python **3.11+** and Node.js **22.22.3**. No Python or npm
packages are required. From the repository folder, run:

```sh
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
python3 -m unittest discover -s scripts -p 'test_*.py'
node --test scripts/test_appearance.cjs
```

[Changelog](CHANGELOG.md) · [Security reporting](SECURITY.md) ·
[MIT License](LICENSE) · [Sponsor DevOption](https://github.com/sponsors/devoption)
