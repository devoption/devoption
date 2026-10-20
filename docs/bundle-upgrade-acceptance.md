# Signed base-bundle upgrade acceptance

On 2026-10-20 the explicit disposable signed-packet rehearsal under
[#263](https://github.com/devoption/devoption/issues/263) passed a real
`0.1.0-preview.1` → `0.1.0-preview.2` upgrade, interrupted activation and full-state
recovery across four distinct kernel boots. No platform binary was rebuilt or
substituted during this rehearsal.

## Exact inputs and environment

| Input | Verified identity |
| --- | --- |
| Previous packet | [Immutable preview.1 record](../release-candidates/0.1.0-preview.1.json), source `a11c0619d98c14f43b41e3947e7f4f00a71dc2f0`, signing run `39182230807`, attempt 1 |
| Candidate packet | [Preview.2 record](../release-candidates/0.1.0-preview.2.json), source `cdc2df021bc518b5314ccf6e47f7a3cc89bf7c8f`, [approved signing/readback run](https://github.com/devoption/devoption/actions/runs/39369686068), attempt 1 |
| Digests and transport IDs | Exact archive/proof/ZIP digests and Actions IDs in those canonical records; neither record is inferred from downloaded receipts |
| Host profile | Fresh Ubuntu 24.04.5 amd64/systemd/cgroup-v2 guest; QEMU software emulation, 2 vCPUs, 2 GiB RAM, private 8 GiB overlay |
| OS and proxy acquisition | Dated Ubuntu image SHA-256 and Caddy 2.11.7 SHA-512 checked against the existing trusted pins |
| Verifier | GitHub CLI 2.102.0; official Linux archive SHA-256 verified before execution; normal signer/source/main/hosted-runner policy |
| Trusted harness | `76e1c20`; the production installer/upgrader CLI files are unchanged from the reviewed merged source |

The successful signer/readback logs and GitHub artifact metadata supplied the
expected candidate identities before download. Local verification compared the
exact ZIP bytes, then the complete packet, cryptographic proof and manifest.
Both packets authenticated again before VM creation and on every guest boot.
The production upgrade CLI reauthenticated both inputs for every invocation.
No caller-supplied verification JSON, unsigned assembly statement, alternate
signer, skip-verification flag or synthetic reservation was used.

## Observed results

| Check | Result |
| --- | --- |
| Corrupt candidate | A modified ZIP failed the production `plan` command at the reviewed transport-digest gate before installer state existed |
| Previous installation | Production install CLI placed the signed preview.1 bundle; real service-identity PTY setup enrolled the disposable owner; HTTPS login and project/profile/Git data were created |
| Supported upgrade | Production `plan`, `apply` and `status` selected the exact preview.2 installation and recorded `healthy`; owner login and saved state worked |
| Completed-upgrade reboot | New kernel booted enabled real services with the preview.2 unit/receipt/executable; prior session was rejected; owner/project/profile/repository state remained usable |
| Full-state recovery | A post-backup sentinel write disappeared from the restored live tree after confirmed production `recover`; preview.1 and the original saved state became usable again |
| Interrupted activation | Attempt 2 injected an exit immediately after the unit write and before the receipt write, leaving a sealed `backed-up` transaction; this is controlled fault injection, not a reproduced OS failure |
| Pending reboot | A new kernel left both services disabled/inactive with ports 80/443/3000 free; the partial transaction remained available for explicit recovery |
| Recovery after interruption | Production `recover` authenticated both packets, restored state and the previous unit/receipt, checked readiness and re-enabled preview.1 |
| Restored-installation reboot | A fourth distinct kernel boot started preview.1; TLS owner login, project/profile/Git state and `restored` status passed; a completed recovery retry preserved a subsequent write |
| Evidence and cleanup | Host validation required all ordered stages, four unique kernel boot IDs, successful signed/production-CLI/corruption checks and exact matching source/target reservation identities. QEMU exited, no QEMU process remained, and the temporary disk/shares/logs and private owner/certificate/session state were removed |

The fixture hostname resolved to loopback inside the guest. Its local Caddy CA
was explicitly installed into that guest's OS trust store, so the production
readiness checks used normal hostname/certificate validation. Owner/API checks
also verified TLS with the same local CA. No host credentials, home directory,
SSH keys or forwarded ports were supplied to the VM. No cloud node or public DNS
record was created. Only the non-secret acceptance report and original signed
input packets were retained locally; private state and raw guest logs are not
part of this document or PR.

## Scope and remaining gates

Reproduce with the [signed-packet runner](platform-upgrades.md#signed-packet-rehearsal).
This establishes the declared same-schema **base bundle** path using real signed
packets and the production CLI. Existing ordinary CI continues to exercise
source-built root/systemd fault cases and real reboot acceptance without requiring
expiring artifact downloads or signing rights on contributor PRs.

This is one local Ubuntu/amd64 profile using a fixture CA. It does not establish
public DNS/ACME upgrade behavior, a separate fresh preview.2 signed installation,
coordinated hosted-app/build state, incompatible migration recovery, hostile
multi-tenant safety or general disaster recovery. App/build opt-ins and migrations
remain refused. Notification/UI, stable/channel/canary thresholds and a promotion
stop mechanism remain open under #263. The manifest/receipt still says
`publication_eligible: false`; acceptance does not rewrite it.

Preview.2's Actions packet has seven-day retention. A preview.2 version tag and
long-lived unpublished draft/asset were not created; that writer needs its
separate explicit approval. The build-time metadata/notes remain byte-identical,
including their then-pending acceptance warning. No product release, package,
production upgrade or repository security-setting change was performed.
