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

On Unix, `serve --data-dir /absolute/private/directory` optionally validates an
existing owner-data directory before listening. It must belong to the current OS
user, have no group/other permission bits, and use a path with no symlink components
or `..`. Invalid owner records stop startup. This option retains the opened
directory but creates no directories or accounts and changes no permissions.
When a valid owner already exists at startup, it enables the HTTP sign-in and
sign-out routes described below. A missing owner leaves them closed; first-owner
setup and browser forms are not implemented. Without this option, the preview
selects no data path.

This local preview binds to `127.0.0.1` and accepts only `127.0.0.1` or `localhost`
at its listening port. Unsafe requests require a matching Origin and a session,
apart from the protected authentication routes. Without a configured owner, all
unsafe requests remain denied. Preview pages remain public. There is no TLS or
complete browser authentication flow. Do not expose it publicly.

For an existing owner, `POST /auth/challenge` issues an HttpOnly pre-session cookie
and a separate `X-CSRF-Token` header. `POST /auth/sign-in` requires that matching
pair and one URL-encoded `password` field; it consumes the challenge before reading
the form and issues a fresh session cookie and CSRF header only after verification.
`POST /auth/sign-out` requires the current session cookie/header pair. All three
routes enforce Host/Origin and return empty, non-cacheable responses. Password
failures return 401, invalid pre-session pairs 403, and admission limits 429/503
with bounded retry hints. Storage/internal failures return 503 without details.

Pre-sessions last five minutes with at most 16 pending pairs. Full capacity returns
503 with `Retry-After` rounded up to the earliest expiry (1–300 seconds); consuming
a challenge may free capacity sooner. Each submission needs
a fresh challenge, including retries after a failed password or invalid form.
Sign-in is limited to one worker and five admitted attempts per minute. One active
session is retained in memory for at most an hour; another sign-in replaces it,
and restarting loses it. Tokens must not enter URLs, logs, or browser-local storage.
First-owner enrollment authorization/policy, browser forms, and recovery of the
CSRF token after a page reload remain unfinished. Temporary stores are used for
tests; there is no supported account provisioning flow yet.

A [local enrollment authorization core](docs/local-enrollment.md) now supports a
locally armed, expiring, single-use browser attempt with bounded audit evidence.
Only temporary fixtures use it. The selected automatic browser flow still needs
a secure handoff, command/HTTP/UI wiring, and enrollment password policy; the
preview does not expose first-owner setup.

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

The Rust workspace has three crates, with dependencies flowing
`devoption-cli` → `devoption-server` → `devoption-ui`:

| Crate | Responsibility |
| --- | --- |
| `devoption-cli` | Arguments, command dispatch, process diagnostics, and service startup |
| `devoption-server` | HTTP/runtime behavior, request guards, authentication, sessions, and owner storage |
| `devoption-ui` | HTML rendering and browser assets |

Server APIs live under `devoption_server`; the CLI is a binary without a library
facade. Auth and storage remain server modules until an independent consumer or
domain boundary warrants another crate. Executable integration tests stay with
the CLI; server unit tests and fixtures stay with the server.

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
