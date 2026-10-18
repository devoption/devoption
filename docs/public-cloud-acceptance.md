# Public single-node acceptance

On 2026-10-17 (America/New_York; 2026-10-18 UTC), an explicitly authorized,
temporary DigitalOcean VM completed the [first-app runbook](first-web-app.md)
for [#234](https://github.com/devoption/devoption/issues/234), including the
public installation and hostname evidence for #230 and #233. The VM and its two
added DNS records were removed afterward. These are recorded results, not a live
demo or a product release approval.

## Environment and source installation

| Item | Observed value |
| --- | --- |
| Platform/runtime source | `b355a8d9fd56e333c9f1f91f2ef08d999c33c644`; its main CI passed before installation |
| Cloud image | DigitalOcean image `247516813`, `ubuntu-24-04-x64`; Ubuntu 24.04.5 x86_64, kernel `6.8.0-142-generic` |
| Node | `nyc3`, `s-4vcpu-8gb-amd`, 4 vCPUs / 8 GiB RAM; fresh OS, owner and stores |
| Rust / Cargo | `rustc 1.99.0 (b940084d7 2026-09-28)` / `cargo 1.99.0 (5f94df478 2026-08-27)` |
| Guest target / runtime | `wasm32-wasip2`, Wasmtime 49.0.3 |
| Caddy | 2.11.7; documented upstream archive SHA-512 verified before installation |
| OS packages | Git `1:2.43.0-1ubuntu7.3`, GCC `4:13.2.0-7ubuntu1`, Python `3.12.3-0ubuntu2.1`, systemd `255.4-1ubuntu8.17`, CA certificates `20260601~24.04.1` |

The unprivileged `builder` account cloned the public repository, checked out the
exact revision, installed its pinned toolchain and fetched locked dependencies.
`cargo build --workspace --release --locked --offline -j 2` then passed in
5 minutes 41 seconds. No prepared platform executable, owner record or app
artifact was copied into this node. The explicit Linux offline registry fixture
also passed using `cargo test --release -p devoption-registry --test offline_build
--locked --offline -- --ignored --nocapture`.

Installed executable SHA-256 values:

| Executable | SHA-256 |
| --- | --- |
| Platform | `33eb64a1a2e5ffbf77e25b672866b02ed90e43fcf6db700db042e70075e0c30f` |
| App worker | `7b41b20f9a869d11dab9823f0f36a99a0a178dbdeb804c3c22586cc87082a4d5` |
| Caddy | `678ade3bfc088749c81a681adc603333ee0bb023b6a6cfe3c0f58bef8ff854e9` |

Installation followed the [public platform](public-linux-install.md),
[build preparation](web-app-builds.md#prepare-the-node),
[deployment](web-app-deployments.md#operator-installation), generated platform
service and [hostname binding](app-hostnames.md) instructions. Dedicated platform
and proxy accounts, private storage, fixed sudo brokers and protected release
storage were used. `visudo`, systemd unit verification and Caddy validation passed.
SSH ingress was restricted to the administrative client; only HTTP/HTTPS were
public. Both backend listeners remained on loopback, and external connections to
ports 3000 and 15000 failed.

Two explicit, 60-second-TTL A records supplied the platform and app hostnames on
one IPv4 address. Both authoritative DNS and the external client's resolver
returned that address. No wildcard, existing DNS record, SSH key or existing node
was changed. The local Chrome browser and Python HTTPS checker used normal public
trust, without test CAs, hosts-file overrides or certificate-validation bypasses.

## Browser and lifecycle results

| Check | Result |
| --- | --- |
| Owner setup and login | Passed hidden-input SSH setup, public HTTPS browser sign-in, project creation and repository attachment |
| Real build A and promotion | Passed interface preparation/approval, offline Wasm compilation, saved diagnostics and immutable release promotion |
| Deploy A | Passed a separate browser deployment review/approval; app homepage showed revision A and `/healthz` returned exact `ok A` |
| Update to B | Passed a second committed source, real build, promotion and deployment; unchanged app hostname showed B and exact `ok B` |
| Failed candidate | Deliberately wrong health expectation returned HTTP 409; UI reported failed / desired stopped and confirmed the candidate was stopped |
| Recover saved A | Fresh browser approval restored A's original saved bytes without rebuilding |
| Reboot while running | Approved A recovered, both HTTPS routes passed, project/two successful builds/two releases survived; old browser session was rejected |
| Reboot while stopped | Platform and proxy recovered; app/build units stayed inactive with empty cgroups, and browser login showed stopped intent |
| Snapshot and restore | Create/verify/stage passed for a 513,539-byte snapshot directory; restored owner saw both builds/releases and freshly approved saved A served publicly |
| Data-preserving removal | Removed the 12 installed binaries/helpers/configuration/unit files; ports 443/3000/15000 closed; 875 retained entries totaling 764,171,512 file bytes had identical contents, owners and modes |
| Reinstallation | Restored the reviewed files/modes; owner, records and renewed certificates survived; app stayed stopped until a fresh saved-A approval, then public checks passed |

The source fixture was the public `examples/web-app` committed as a standalone
repository. B changes only the homepage revision and health body. Both components
were 123,606 bytes:

| Revision | Fixture commit | Component SHA-256 |
| --- | --- | --- |
| A | `794c9e9991d432780a941f64c6705410e649c289` | `d085e733cfdb994e7541c3333a8f12220098bef984d7707310b2425c1e8b33f1` |
| B | `25b02de94521ba2e5d6b10e1934c12b1de250318` | `fed77afa2dfe1ae1860296a4616cae30083a48a99067e39c28941a1491a76f0b` |

Saved release A remained
`86b3ae13d0062332acbdb82f0a3f153eac4602c34d20457cab78e9278e895ef3`;
B remained `08891a28e3b0238817a46db57560d3ad5be05fb18ef2662dd935a3c556a496ff`.
No new app build was needed for rollback, reboot, snapshot adoption or reinstall.

![Saved A serving publicly after reinstallation](screenshots/public-cloud-release.png)

## External routing and certificate lifecycle

Each successful external report ran `scripts/check_public_app.py` against the
same two hostnames and expected IPv4, selecting the exact health body for that
phase. All 12 checks passed: both DNS answers, trusted platform sign-in,
unauthenticated project redirect, unknown HTTP Host rejection, exact app health,
five reserved control paths and app-SNI/platform-Host rejection.

| Phase | Successful report time (UTC, 2026-10-18) |
| --- | --- |
| A | 01:32:29 |
| B | 01:33:26 |
| Saved-A recovery | 01:34:26 |
| Running reboot | 01:34:47 |
| Staged restore | 01:35:58 |
| Reinstallation | 01:36:30 |

Additional checks rejected unknown TLS SNI. Browser/session checks confirmed
host-only Secure/HttpOnly/SameSite=Strict cookies, no platform cookie at the app
hostname, HTTP 403 for an app-origin mutation, HTTP 401 for a mutation without
CSRF, and HTTP 303 to sign-in for anonymous project access.

Let's Encrypt issued publicly trusted certificates for both names. To exercise
actual renewal during the short run, the operator temporarily used Caddy's
documented `renew_interval 1m` and `renewal_window_ratio 0.999999`. Caddy logged
`certificate renewed successfully` for both names at 01:30:52 UTC, and external
TLS checks confirmed changed serial numbers. The original generated Caddyfile
was restored byte-for-byte immediately afterward. The renewed certificates then
survived both reboots and reinstallation. This exercises real ACME renewal with
an accelerated threshold, not months of unattended operation. See the
[Caddy renewal options](https://caddyserver.com/docs/caddyfile/options#renewal-window-ratio).

## Corrections, cleanup and limits

The first external checker run rejected the successful 90,219-byte sign-in page
because its own cap was 64 KiB. The checker now permits at most 128 KiB; a real
HTTP-decoding regression test covers that page size, the cap and overflow. All
six successful reports above use this correction; the installed platform/runtime
remain the pinned source revision above.

Operator setup also encountered Git's checkout-owner guard and group-writable
Git metadata. Recording the source revision as its build user and removing
group/other write permission from fixture Git metadata resolved them without
disabling either guard. Caddy rejected renewal ratio `1` during test setup;
the validated exclusive-range value above was used instead. Initial probes also
observed brief 502/connection failures during startup: systemd active state was
never counted as route readiness, and acceptance required actual HTTPS responses.

After Stop app and evidence collection, cleanup deleted only the two recorded
new DNS record IDs and this run's Droplet. The full DNS record inventory matched
the pre-run snapshot exactly. DigitalOcean subsequently returned 404 for the
Droplet; deletion was asynchronous, so its initial acknowledgement alone was not
counted as absence. Existing nodes and SSH keys were untouched. Temporary local
owner-password and browser-session files were deleted. Private raw logs, DNS
inventories, snapshots and credentials are not included in this public report.

Repository validation passed the workspace build and tests (493 passed, two
explicitly ignored), formatting, Clippy with warnings denied, 80 Python tests,
101 Node tests and `git diff --check`. The Linux offline fixture was run explicitly
on the cloud node as recorded above. The public diff was reviewed for private
paths, credentials, raw infrastructure inventories and unsupported claims.

This validates one trusted stateless Rust/WASIp2 fixture, one Ubuntu x86_64 node,
one provider/region and IPv4. IPv6, arbitrary apps/frameworks, databases, hostile
multi-tenancy, portable/off-host disaster recovery and sustained public-load
testing were not performed. The snapshot needs the original installation's
repositories, inputs, executable/configuration and Caddy state. Functional
acceptance does not authorize merging, tagging or publishing v0.1.0; #21 remains
the separate owner-controlled release gate.
