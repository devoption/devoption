# Clean local installation and maintained-app rehearsal

On 2026-10-17, a new disposable Ubuntu VM completed the local browser path in
[the first web-app runbook](first-web-app.md), using new owner/catalog/build/release
storage. The previous rehearsal VM was preserved and stopped. This is evidence
for [#234](https://github.com/devoption/devoption/issues/234), not completion of
its public cloud/DNS acceptance criteria.

## Environment and provenance

| Item | Recorded value |
| --- | --- |
| OS | Ubuntu 24.04.5 x86_64 minimal cloud image, release `20261008` |
| Image SHA-256 | `2f119f50bf29fe40e73b69ae2a736e8210d8146796f036e0324315b6b3a777df` |
| VM | QEMU TCG, 2 CPUs, 2 GiB RAM, fresh 6 GiB overlay |
| Installation scripts/examples | `243d6a519262e8845782a4cafda03291b9d730fd` plus the proxy timeout correction below |
| Installed Linux binary source receipt | `5c33bf6d186e43508ec238ed9b3fd0c1101d11e0` |
| Platform binary SHA-256 | `8a4a400419e53f62c8f21b1b4b7f41d43529e69533ba1e2eb7524e1179f4cae5` |
| App worker SHA-256 | `fa388c551d0f1c16aa697943d4cec9ed2481c21859354d130595a25e7e111e1a` |
| Caddy | 2.11.7, binary SHA-256 `678ade3bfc088749c81a681adc603333ee0bb023b6a6cfe3c0f58bef8ff854e9` |
| Toolchain | Rust 1.99.0 with `wasm32-wasip2`; prepared vendor/archive inputs |
| Packages | Git `1:2.43.0-1ubuntu7.3`, GCC `4:13.2.0-7ubuntu1`, Python `3.12.3-0ubuntu2.1`, systemd `255.4-1ubuntu8.17`, CA certificates `20260601~24.04.1` |

The platform and worker executables, toolchain and authenticated dependency inputs
were reused from the earlier source-built Linux fixture. The Rust tree, Cargo
manifests/lockfile and toolchain pin were unchanged between that binary receipt
and the installation-script revision. This is a **fresh OS/state installation
with prepared binaries**, not proof of compiling the platform on a fresh VM.
The Overview wording correction in this PR was built and captured separately in
the host browser fixture; it was not present in the installed Linux executable.

No owner, project, build or release records were copied into the new installation.
Interactive owner setup reused the existing disposable rehearsal password; no new
secret was generated. Accounts and storage used the documented dedicated
`devoption` identity and conventional `/opt/devoption`, `/etc/devoption`,
`/var/lib/devoption` and root-owned `/var/lib/devoption-app` paths.

The [platform install](public-linux-install.md), [build preparation](web-app-builds.md),
[deployment install](web-app-deployments.md) and generated platform service were
applied in that order. `systemd-analyze verify` and Caddy validation passed.
Transferred public toolchain/vendor files initially inherited a restrictive umask;
restoring their read/execute access allowed the dedicated account to validate them.
This was an input-transfer permission error, not a bypass of input verification.
The initial local seed image also needed a plain ISO/Joliet layout before cloud-init
could read it; only the unused new overlay was recreated.

`platform.example.test` and `app.example.test` used one Caddy HTTPS listener via an
isolated SSH tunnel/browser proxy. Test-only internal TLS, loopback binding and an
explicit rehearsal CA were used as documented in [local hostname validation](app-hostnames.md#local-validation).
No public DNS, system trust, cloud resources or public certificate issuance changed.
Browser trust was scoped to the rehearsal certificate; separate TLS probes validated
the full chain and hostname against its explicit root certificate.

## Observed results

| Check | Result |
| --- | --- |
| Fresh owner login and project | Passed through HTTPS; created First web app and attached its standalone example repository |
| Build A, save release, deploy | Passed through browser review/approval; real WASIp2 compilation succeeded in 14.89 seconds; app returned `ok A` |
| Failed health check | Candidate stopped and desired intent became stopped; initial proxy timed out incorrectly, then corrected configuration returned the platform failure response |
| Second hostname | Open app displayed revision A; UI verified the route with the explicit rehearsal CA |
| Update A → B → saved A | Both real builds succeeded; UI promotion/deployment served `ok B`, then `ok A` at the unchanged hostname without rebuilding A |
| TLS/routing separation | Passed platform sign-in and unauthenticated project redirect, exact app health, blocked control paths, SNI/Host mismatch and unknown-host rejection |
| Full VM reboot while running | Saved A recovered; both URLs, owner login, two successful build records and both saved releases remained usable; prior browser session was rejected |
| Reboot after Stop app | Platform and Caddy started; app and build units remained inactive |
| Snapshot/staged restore | Create, verify and stage passed; approximately 0.49 MiB including manifest; restored owner logged in through HTTPS, saw both builds/releases and deployed saved A with a fresh review |

The app source was the committed public `examples/web-app` copied into its own
repository. Revision B changed only the homepage and health response:

| Revision | Source commit | Component SHA-256 |
| --- | --- | --- |
| A | `4e9471edcb9ca330f67f1462ad9ebc04c81bd116` | `d085e733cfdb994e7541c3333a8f12220098bef984d7707310b2425c1e8b33f1` |
| B | `be20b656b7d99b61f6a8da246856ae2c91a759a1` | `fed77afa2dfe1ae1860296a4616cae30083a48a99067e39c28941a1491a76f0b` |

Saved A retained release ID
`4aebab0346cb19a064aa4fff3f53ddc8a171ad768ca11336749e6b906f0e5494`
and B retained
`125b229f2b5b31de46f985c2ca89a13ac7704dd0941d08da602234ca9ca3e2db`.
Snapshots and restored trees remained private. Recovery used the documented
[same-node staging procedure](same-node-recovery.md), preserving the original
repository/input identities. Original configs and service were reinstated after
stopping the restored app. No app compilation was required for recovery.

## Failure found and corrected

Before app-hostname configuration, the platform-only Caddy example allowed only
15 seconds for response headers. A deployment with deliberately wrong health
returned HTTP 504 after 15.042 seconds while its bounded health check continued.
The candidate eventually stopped, but the browser could not receive its result.

The initial example now uses the same 95-second response allowance as the generated
two-host configuration. Repeating the same failure returned the platform's HTTP
409 after 33.816 seconds, and refresh showed failed/stopped intent. A fresh review
with `ok A` recovered normally. This does not make deployment transactional or
remove update downtime.

## Data-preserving removal and reinstallation

After Stop app, the original installation was selected again. Its platform and
Caddy services were disabled/stopped; the fixed build, app and validation units
were confirmed inactive. A private root-only backup retained these 13 installed
files with their original modes before removal:

- `/opt/devoption/devoption` and `/usr/local/bin/caddy`.
- `/usr/local/libexec/devoption-{app-worker,build-broker,build-worker,deployment-broker}`.
- `/etc/devoption/{build.toml,deployment.toml,rehearsal-ca.crt}` and `/etc/caddy/Caddyfile`.
- `/etc/sudoers.d/devoption`.
- `/etc/systemd/system/{devoption-public,caddy-devoption}.service`.

These are the exact paths for this disposable installation, not a wildcard removal
recipe for another node. After daemon reload, both service definitions were absent
and ports 443, 3000 and 15000 were closed. SHA-256 inventories, owners and modes
matched before/after for the original data/repositories/history/jobs, root release
store, Caddy state, prepared inputs, snapshot and restored tree. Accounts, source
receipt and all retained directories remained in place.

The same files/modes were reinstalled; sudoers validation passed and both services
were enabled/started. Existing owner login and both successful build/release
records survived. The app stayed stopped until a fresh browser deployment review
of saved A; it then served `ok A` through the same verified HTTPS route. No setup,
new owner, new app build or certificate replacement was needed.

The rehearsal app was stopped and both VMs were shut down afterward. Temporary
browsers, tunnel and proxy were closed; original/new disks and private recovery
artifacts were retained. Combined allocated artifacts peaked at approximately
5.25 GiB under the explicitly approved 8 GiB cap, with one-second disk monitoring.

## Limits

Public A/AAAA routing, externally trusted ACME issuance/renewal, Internet access
and fresh platform compilation remain unperformed in this pass. Local hostnames
and a test CA cannot establish those results. The supported app is still one
trusted stateless Rust/WASIp2 app; this does not validate arbitrary frameworks,
databases, app authentication, portable restore or disk-loss recovery.
