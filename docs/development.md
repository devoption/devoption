# Development

Follow the [contribution guide](../CONTRIBUTING.md) and install the
[quick-start prerequisites](../README.md#quick-start) first.

## Architecture

See the [architecture map](architecture.md) for crate responsibilities, implemented
data flow and planned boundaries. Focused references are in the
[documentation index](README.md).

## Project-page rendering

The [#22 audit](https://github.com/devoption/devoption/issues/22) found repeated
identity definition rows in build history and deployment details, and repeated
live action-status paragraphs in build and deployment review. Small private Rust
helpers in `crates/devoption-ui/src/projects/render.rs` own those fragments. They
accept text and escape each value, label and attribute; callers must not pass
pre-escaped content or raw HTML. Identity rows retain semantic `dt`/`dd` and
`code` markup. Review status paragraphs retain their existing IDs, `role="status"`
and `aria-live="polite"` for the browser action handlers.

The audit also covered project/repository forms, source checks, saved build states
and deployment controls. Form fields have different constraints, autocomplete,
help/error relationships and JavaScript hooks; they stay with their page renderers.
Build-state badges, source check outcomes and deployment health text have different
structures and meanings. Keep those local too. Page-specific controls still own
their action attributes, disabled state and approval boundaries. There is no shared
form schema, status-policy mapper or new component framework.

This extraction preserves rendered output, escaping, semantic labels, keyboard
controls, focus targets, visible status text and theme tokens. A before/after
comparison of 28 build/release/history page variants was byte-for-byte identical,
so existing screenshots remain applicable. Any later visible UI change still
requires the captures in the contribution guide.

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

Python checks include the [release readiness and canary fault matrix](release-readiness.md#validation-and-remaining-scope).
Its simulated observation clock tests policy boundaries; it is not live canary evidence.

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

Rust formatting/Clippy, workspace tests and the debug build (including the
explicit trusted offline fixture), and Python/Node checks run on every PR.
A separate selection job compares the PR head against its merge base with the
current PR base, using full Git history and a NUL-delimited diff without rename
detection. This includes both sides of moves and avoids API file-count limits.

The expensive release build and installation matrix is skipped only when every
changed path is in `docs/`, `crates/devoption-ui/assets/`, or is a root `README.md`,
`CHANGELOG.md`, `CONTRIBUTING.md`, or `AGENTS.md`. Shipped bundle inputs
(including installation instructions under `docs/` and `LICENSE`) always require
installation coverage; the routine changelog is the sole bundle-input exception.
Rust source (including
UI rendering), manifests/lockfiles, toolchains, scripts, workflows, examples,
release inputs and all unknown paths require installation coverage. Missing,
unreadable or empty diffs also require it. The selection job reports its decision
in the Actions summary. This is a conservative path policy, not dependency analysis.

Pushes to `main` always run full validation. Reusable workflow calls default to
`full-validation: true`, preserving full acceptance for release checking,
planning, provenance and candidate retention, even if called from a PR context.
No release workflow caller is changed to opt out.

The required `Rust and repository checks` status requires successful selection,
lint, tests and scripts, plus successful installation when selected. It accepts
installation's `skipped` result only for an explicit successful skip decision;
missing decisions, failures, cancellation and other skipped jobs fail the gate.
Each validation job checks for tracked-file changes. `Issue and change
documentation` remains a separate required status. Repository settings are unchanged.

Installation coverage runs in four parallel suites on separate disposable runners:
`bundle` covers source installation, proxy, packaged runtime, named apps and fresh
VM installation; `records` covers HTTPS and scoped/managed PostgreSQL;
`upgrade` covers prior-preview upgrade, recovery and four kernel boots; `apps`
covers independent app deployments and their three kernel boots. Each suite builds
its own locked release workspace and has a 15-minute job deadline. This repeats
compilation to reduce elapsed time without transferring compiled artifacts between
runners. Matrix fail-fast is disabled so a failed suite does not cancel the others.
The required gate consumes the combined matrix result and requires every selected
suite to pass. VM tool installation has a three-minute step deadline, two apt
download retries, 30-second HTTP/HTTPS timeouts and a 60-second package-lock wait.
These bounds fail stalled setup promptly; they do not turn failed acceptance into
a passing check.

The bundle suite downloads Caddy using the existing exact version/checksum
pin and explicitly runs the ignored `public_https_proxy` test. It uses loopback
ports and temporary server/client certificates without installing trust or host
services. Both ordinary and protected access are checked, including certificate
verification/leaf authorization, real owner login, revocation after proxy restart,
forged-header/cross-host rejection and independent public-app routing.

The bundle suite also assembles a local preview bundle from the release
executable, runs its version/help checks, and compares every archived byte and
manifest entry, source-inventory SBOM and unsigned assembly statement with the
trusted checkout/build inputs. It uploads no artifacts;
see [bundle candidates and remaining trust gates](release-bundles.md).
It also builds the public locked WASIp2 fixture and runs
`check_bundle_runtime.py` against the separately packaged worker snapshot: version,
component validation, owned-invocation health, routing and bounded process cleanup.
The worker's digest/mode and schema-3 descriptor are verified separately; historical
schema-2 packets stay supported without inventing a runtime member.
It then runs `python3 scripts/check_bundle_installer.py OUTPUT` against that
trusted local assembly: disposable file placement, rendered-unit verification,
real PTY owner setup, installed-binary readiness/restart and a state-preserving
reinstall. No system accounts, host units or services are changed. Signature
failure/acceptance policy is tested separately; real privileged installer and
reboot acceptance remain subject to the [installer's validation limits](bundle-installation.md#validation-scope).

The records suite runs the [scoped HTTPS proof](app-https.md#disposable-proof-and-limits)
with a real WASIp2 guest, disposable CA and synthetic bearer token. Managed Linux
acceptance covers systemd credentials and exact recovery with both worker identities.
Neither checker contacts a real integration provider.

The bundle suite additionally runs `check_named_web_app.py` with explicit
`--disposable-system-host`. It refuses existing build helpers/configuration,
prepares and verifies the public static site's offline dependencies, temporarily
installs the real build broker/worker, and builds two independently named-app
revisions inside the existing systemd limits. It verifies exact CSS/JavaScript
responses and A → B → saved A through the locally built runtime, then stops the
unit and removes its installed files. This proves compiler/runtime compatibility,
not full project UI, immutable-release promotion, HTTPS or reboot acceptance.
Never run this privileged fixture on an existing installation or workstation.

The records suite builds checksum-pinned PostgreSQL from official source in runner
temporary storage and runs [the scoped record proof](app-records.md#disposable-local-proof)
using a generic WASIp2 fixture. The first proof uses a private Unix socket and no
host service or login password. A second, explicitly disposable Linux checker
then installs the real broker, creates only its reserved non-login app/database
test identities and private systemd services, and verifies managed grants,
revocation/restart refusal and saved stateless recovery. It refuses existing
deployment files/units/identities and removes only its fixture resources after
stopping them. No remote database or production secret is used.

The upgrade suite also builds the exact reserved prior source and a disposable
next-preview source fixture with real versioned executables, then runs
`check_bundle_upgrade.py` on its fresh GitHub-hosted Ubuntu runner. Its explicit
`sudo` mode creates real dedicated service identities, units, state and a local
test CA, exercises the upgrade engine and full-state recovery, and removes those
resources. It verifies owner/project/profile/repository persistence, interrupted
activation, disabled boot during incomplete transactions, controlled failed-health
recovery and owner-only upgrade observations in the candidate server. The latter
checks exact transaction/archive identities and refusal of browser mutation
requests. This step is destructive to the fixture's reserved host paths: **never
run it on an existing installation or workstation**. It uses no repository secrets,
production credentials, signing/publication rights or self-hosted runner. Trusted
source builds establish fixture identity; production packet verification has no
fixture bypass. See [upgrade scope and remaining acceptance](platform-upgrades.md).
The hosted runner's writable `/opt` parent is temporarily made root-owned mode
0755 during this explicit fixture, then restored to its original UID/GID/mode.
Production installation/upgrade checks continue to reject writable ancestors.

The upgrade suite installs QEMU/xorriso and runs `check_upgrade_reboot.py --fixtures
FIXTURE_DIRECTORY --report NEW_REPORT_PATH` in an isolated guest. It verifies the
SHA-256-pinned, dated Ubuntu image, creates a private disposable disk and performs
four kernel boots: initial upgrade, completed-upgrade verification/interrupted
activation, pending-transaction verification/recovery, then restored-installation
verification. It checks TLS login and persisted project/profile/Git data, exact
selected version, disabled pending services, and recovery retries. KVM access is
granted only to the ephemeral runner user when available; otherwise QEMU uses
software emulation. All boots share a ten-minute deadline. Missing/failed evidence
or repeated boot IDs fail the required upgrade suite. The runner kills/waits
for a timed-out guest and deletes its temporary disks/shares/logs. This creates no
cloud resources, exports no credentials, forwards no network ports and retains no
owner/certificate data. The small JSON report records only stages, versions and
kernel boot IDs. An explicit [signed-packet rehearsal](platform-upgrades.md#signed-packet-rehearsal)
uses the production verifier/CLI against independently reviewed packets; ordinary
PR CI keeps source-built fixtures and requires no artifact access or signing rights.
See [signed acceptance and remaining limits](bundle-upgrade-acceptance.md).

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
