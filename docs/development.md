# Development

Follow the [contribution guide](../CONTRIBUTING.md) and install the
[quick-start prerequisites](../README.md#quick-start) first.

## Current architecture

The Rust workspace has six crates. The running service uses
`devoption-cli` → `devoption-server`, which composes `devoption-projects` and
`devoption-ui`. The project domain core is independent of those delivery layers:

| Crate | Responsibility |
| --- | --- |
| `devoption-builds` | Immutable Unix build plans, in-memory job lifecycle records and bounded Cargo source membership proofs, with no approval or execution |
| `devoption-cli` | Arguments, command dispatch, process diagnostics, and service startup |
| `devoption-projects` | Project/repository identities, associations, private catalog persistence, and bounded worktree inspection and explicitly managed creation |
| `devoption-registry` | Cargo input validation, dependency metadata, checked provider contract and verified single-crate seed bytes; no acquisition service |
| `devoption-server` | HTTP/runtime behavior, request guards, authentication, sessions, and owner storage |
| `devoption-ui` | HTML rendering and browser assets |

Server APIs live under `devoption_server`; the CLI is a binary without a library
facade. Authentication and owner storage remain server modules. The
[project catalog core](project-catalog.md) has its own model and storage
tests. Its separate [worktree inspector](worktree-validation.md) accepts
bounded candidates under an explicit trusted local root, with opt-in backend
attachment tested using temporary fixtures. The server validates the catalog through
the same opened directory as owner storage at startup, then rereads it for each
authenticated Projects GET/HEAD request and supplies
current display summaries to the UI. Project creation, managed repository creation and repository attachment
use the domain's locked, durable save operations. Executable integration tests stay with the CLI; domain
logic remains in the project crate. The separate [build-plan model](build-plans.md)
validates explicit inputs against a catalog snapshot without I/O and has no runtime
caller. It includes the [registry dependency descriptor](registry-contract.md):
the [supplied-byte Cargo adapter](cargo-inputs.md) validates a strict standalone
manifest/lockfile subset and binds exact input hashes without reading a repository.
The separate [root source verifier](cargo-source-proof.md) checks a bounded
supplied Git object chain linking those bytes to a selected commit ID. It does not
establish trusted revision selection, complete source context or build permission.
Plans retain this proof and reject a mismatched commit or dependency request.
DevOption-mediated acquisition and isolated offline builds are the selected
architecture, while registry hosting, fetching, approval, execution, environment
policy and resource enforcement remain unimplemented.
The [offline fixture proof](offline-fixture-build.md) uses a concrete local
seed provider and real Cargo in Linux tests. This does not enable application builds
or establish production execution/isolation permissions.
The [artifact handoff](artifact-handoff.md) is fixture-tested; stopped-runner
integration and VM validation remain pending. See the [documentation index](README.md)
for bounded rehearsal results and execution limits.
The [job lifecycle model](build-jobs.md) binds records to an exact plan and
attempt and rejects invalid transitions. Starting a job and recording runner
outcomes require sealed events; queued cancellation is local. No production event
issuer, persistent jobs or runtime integration exists yet.

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

On Linux, also run the explicit trusted fixture build (GNU `timeout` and the
installed Rust toolchain/linker are required):

```sh
cargo test -p devoption-registry --test offline_build --locked --offline -- --ignored --nocapture
```

