# Operator-triggered preview upgrades

The first upgrade path under [#263](https://github.com/devoption/devoption/issues/263)
is a **same-schema base bundle installation** on dedicated Ubuntu 24.04 amd64.
`scripts/upgrade_bundle.py` authenticates both packets, records a bounded offline
recovery point, activates the candidate and checks private/public readiness.
It has no publication or unattended-upgrade operation. Operator approval of a PR
is not approval to run it on an existing server.

The retained `0.1.0-preview.1` packet remains fresh-only. The separately approved
preview.2 packet and [real signed upgrade/reboot acceptance](bundle-upgrade-acceptance.md)
establish the base preview.1-to-preview.2 path. Both exact packets are preserved in
long-lived unpublished validation drafts; see [retention evidence](release-reconciliation.md#live-retention-evidence)
and the [candidate catalog](release-candidates.md). Ordinary CI still builds real
previous/next source executables as disposable fixtures; these are not signed
release artifacts.

## Supported compatibility boundary

A candidate must use [release metadata schema 2](release-metadata.md), declare
`installation = "fresh-and-upgrade"`, and list **one explicitly reviewed earlier
preview of the same base version** in `upgrade_from`. This permits an explicitly
declared path past a failed unpublished candidate; no skipped path is inferred.
Both `data_schema` and
`configuration_schema` must be integer `1`, `migration.required = false`, and
`recovery.binary_rollback = false`. The legacy reserved `0.1.0-preview.1` schema-1
packet is the initial schema-1 data/configuration baseline. Other schema-1 source
versions are not inferred compatible.

The hostname, dedicated accounts, proxy acquisition pin and configuration must
match the existing installation. Only the platform unit's versioned executable
path can change. Source installations, custom unit/configuration conversions,
separate state mounts, build/app opt-ins and migrations are refused before
activation. Coordinated hosted-app/worker upgrades and sample-app acceptance
remain open under #263. Do not delete an opt-in or rewrite a receipt to bypass
these checks.

Both independently reviewed reservations and original candidate ZIPs are required
for **every command**, including recovery. Use the pinned verifier/proxy and
trusted-checkout preparation from [bundle installation](bundle-installation.md).
Offline means the services are stopped; packet verification can still require
outbound HTTPS for the verifier's trust material.
No candidate scripts or saved transaction commands run as an updater. Files are
reconstructed from freshly verified packet bytes; the journal binds exact source,
target, hostname and attempt identities. Old version directories are retained.
Only the supplied previous/candidate version trees are admitted in `/opt/devoption`.
Before a later pair, archive older terminal version trees outside that inventory,
preserving their verified inputs and transaction backups. Unknown or conflicting
trees stop preflight; the updater never deletes them or reuses a failed version
for different bytes. Older recovery points need their original version tree restored
to its recorded location before use. Automatic history/pruning is outside this path.

## Plan and apply

Run from the reviewed trusted checkout on the explicitly authorized host. First
verify owner login, saved projects/profile and repository access through public
HTTPS, and arrange a maintenance window. Stop other administrative mutations.
The existing base services must be active and publicly healthy before a new
attempt. A plan checks authenticity, manifests, installed bytes/ownership/modes,
configuration, accounts, ports, storage and current private/public readiness.

```bash
upgrade_args=(
  --previous-reservation /trusted/previous-reservation.json
  --previous-packet /private/previous-packet.zip
  --candidate-reservation /trusted/candidate-reservation.json
  --candidate-packet /private/candidate-packet.zip
  --gh /usr/local/bin/gh
  --caddy-archive /private/caddy.tar.gz
  --hostname platform.example.com
  --allow-validation-candidate
  --attempt 1
)
sudo python3 scripts/upgrade_bundle.py plan "${upgrade_args[@]}"
sudo python3 scripts/upgrade_bundle.py apply "${upgrade_args[@]}"
sudo python3 scripts/upgrade_bundle.py status "${upgrade_args[@]}"
```

Use the independently reviewed catalog records and their exact original signed
ZIP packets, as in the [accepted signed rehearsal](bundle-upgrade-acceptance.md).
Do not substitute unsigned CI fixture inputs into the production CLI.
`--allow-validation-candidate`
continues to be mandatory and never changes publication eligibility.

All modes share the installer's root-only host lock. `apply` disables boot activation
and stops both platform and proxy, confirms inactive state/empty cgroups, snapshots
state and configuration, verifies the copy, then commits new immutable versioned
files and atomically switches the platform unit and receipt. A version probe runs
as the service identity. Services start without boot activation; only successful
private readiness and normally trusted public HTTPS checks record `healthy` and
re-enable them for boot. An unfinished activation therefore remains disabled after
a reboot. An interruption before service shutdown leaves only the old installation
eligible to boot.

Private checks require `/projects` to redirect to `/sign-in`; public HTTPS must
verify its certificate/hostname and return sign-in 200 and the project redirect.
They do not prove owner login or all saved application behavior. Repeat owner
login, project/profile/repository checks and reboot verification in the acceptance
window. `status` reports recorded phase/version and remaining payload files; its
`healthy` phase is a historical transaction result, not a current login guarantee.
Identical completed `apply` retries recheck/restart the selected installation and
finish boot enablement without taking another backup or replacing saved data.

## Owner notification and operator action

Builds containing this interface show **DevOption updates** on the signed-in
Overview page, after the project action. The page reports the running binary's
version and the administrator-selected candidate, recorded phase, attempt,
transaction and archive/source identities. Expand **Review upgrade and recovery
steps** for the SSH workflow above. The operator runs the existing production
`plan`, `apply`, `status` and confirmed `recover` commands with the full reviewed
arguments; the web service gains no service-control or sudo capability.

The production upgrader writes a small root-owned observation at
`/var/lib/devoption-updates/notice.json` after a successful preflight and each
journal phase. It creates a root-owned mode-0755 directory and atomically replaces
its mode-0644 file under the existing installer lock. Unsafe destinations stop
preflight before service/data mutation. The record contains only version/digest,
transaction, attempt, phase and observation time; it contains no hostname, command,
packet path, owner state, password or backup contents. Keep the private journals
and backups under their existing mode-0700 installer directory.

The server reads only this fixed observation on an authenticated page request,
using bounded, descriptor-relative, no-follow reads through root-controlled
parents. Unsafe ownership/modes, links, oversized/noncanonical/unknown records
or future timestamps display unavailable status without host-path errors. It
never reads the private journals. Missing records display **No reviewed update**;
this is not a claim that the server is current. Non-Linux source previews have no
installed-host observation. The displayed version comes from the running binary,
not the notice. A different version/result or an observation older than 15 minutes
asks for a fresh SSH check; recorded success is not continuous health monitoring.
Use **Refresh status** to reload the saved observation, not to discover, download
or activate a candidate. No background release feed or unattended update runs.

The previously retained preview.1 and preview.2 executables predate this interface.
Updating the trusted checkout can write observations but cannot add UI to those
immutable binaries. A later reviewed candidate needs a new version and its own
signing/acceptance; do not replace either retained packet. The current installed
base-upgrade boundary still rejects app/build opt-ins and migrations.

## Recovery point and full-state restore

Transactions live under `/var/lib/devoption-installer/upgrades/TRANSACTION_ID/`
in administrator-owned mode-700 directories. Their mode-600 journal and sealed
backup manifest bind the plan and all recorded bytes/modes/UIDs/GIDs. The offline
copy contains the complete `/var/lib/devoption` and `/var/lib/caddy` trees plus
original units, Caddyfile and installer receipt. This includes owner/password
records, projects/profile, repositories and private certificate material: protect
it as credentials, never post it in issues or logs, and copy it separately for
off-host disaster recovery. Retain both verified packets and proxy input too.

The initial snapshot limit is 256 MiB and 20,000 entries. Links, special files,
special permission bits, pending/tmp entries and separate state filesystems are
unsupported. Preflight requires the existing conservative 1 GiB reserve plus
backup/restore/payload space. This is a deliberately bounded recovery profile;
large repository installations need a separately reviewed capacity/backup path.
No backup or previous release is automatically pruned.

If activation, health or shutdown fails, preserve the error and transaction. The
command tries to stop/disable both services and records `failed`; uncertain
shutdown never proceeds into backup/activation. A hard interruption can leave
`prepared`, `backed-up` or `activated`. Use the same authenticated inputs and
attempt for explicit recovery:

```bash
sudo python3 scripts/upgrade_bundle.py status "${upgrade_args[@]}"
sudo python3 scripts/upgrade_bundle.py recover "${upgrade_args[@]}" \
  --confirm-discard-post-backup-data
```

**Recovery causes downtime and removes post-backup writes from the live view.**
The confirmation flag acknowledges that loss. Recovery verifies the sealed backup,
confirms services stopped, stages a byte/ownership/mode-verified copy, preserves
displaced live trees under its private `restore-*` directory, restores both state
trees, reinstalls the old verified unit/receipt and checks the previous service.
The previous services are enabled for boot only after readiness passes. Displaced
data remains available for manual inspection; it is not merged into restored state.
This is full backup/restore recovery, not a promise that swapping an old executable
can reverse incompatible data migration.

An interruption between unit/receipt writes or between live-tree renames can be
recovered with the same command. Recovery retains partial staging/displaced trees
and allows at most four restore staging attempts before manual inspection. An
interruption during the initial directory/journal reservation, missing/corrupt
backup or unknown filesystem state may require manual inspection; do not delete
or guess ownership of it. Before a backup is sealed, recovery can restart the
unchanged previous installation only if its exact unit and receipt still match.

A completed recovery retry preserves subsequent live writes. To try the candidate
again afterward, choose a new `--attempt` so a new offline backup captures current
state. Unfinished attempts block new attempts; history is capped at 100 entries
until an administrator archives verified terminal records separately.

## Automated evidence and remaining work

Python fault fixtures cover supported/unsupported version/schema/configuration
paths, corrupt packets before host mutation, disk/stop refusal, backup corruption,
interrupted payload and unit/receipt commits, interrupted directory replacement,
failed candidate checks, private journal validation and full-state recovery.

The separate explicit disposable-host CI checker builds the reserved previous
source and current source stamped as the next preview, assembles both archives,
and exercises the production installation/upgrade engine on a fresh GitHub-hosted
Ubuntu runner with real root account/filesystem/systemd operations. It uses a
per-connection local test CA for HTTPS and a disposable owner; it checks real
owner/project/profile/repository persistence, restoration, interrupted activation,
boot disablement and a controlled candidate-health rejection. It cleans its own
accounts, units and state afterward. The runner's writable `/opt` parent is
temporarily root-owned mode 0755 and its
original metadata is restored after cleanup. Production parent checks stay strict.
Ordinary CI permissions remain read-only; there are no production credentials,
self-hosted runners, signing or publication
steps. See [development checks](development.md#continuous-integration).

The first complete [Ubuntu CI run](https://github.com/devoption/devoption/actions/runs/39334095577)
passed this real-system fixture at 22:22 UTC on 2026-10-19, including cleanup.
It also passed all 258 Python tests, 101 Node tests and required Rust/install
checks at that revision. Current PR checks validate subsequent changes.

CI additionally runs `scripts/check_upgrade_reboot.py` against those same two
source-built bundles inside a fresh private QEMU Ubuntu guest. It checks a
completed upgrade after reboot, interrupts a second attempt between unit and
receipt writes, reboots and confirms both services remain disabled/inactive with
their ports free, then explicitly recovers and reboots the restored installation.
Owner login, session invalidation, project/profile/repository persistence, the
running unit's executable version and installer/transaction records are checked.
A completed recovery retry must preserve subsequent writes. Four distinct kernel
boot IDs and every ordered stage are mandatory; timeout, missing/failed results
or a repeated kernel identity fail CI.

The runner downloads the [official dated Ubuntu minimal image](https://cloud-images.ubuntu.com/minimal/releases/noble/release-20261017/)
and checks its reviewed SHA-256 before making an 8 GiB copy-on-write disk. It
copies only fixture bundles, the pinned proxy and Python scripts into a read-only
guest input share, with a separate private result share. No host home, credentials,
SSH keys or forwarded ports are supplied. Cloud-init installs Git/sudo and a
fixture-only boot service; the production updater is unchanged. Each guest reboot
exits QEMU and starts a new kernel against the same disk. The host runner bounds
all four boots to ten minutes, waits for QEMU termination and deletes the temporary
disk, shares, owner/certificate state and logs on success or failure. Only a small
non-secret JSON acceptance report survives. Never use the destructive `--guest`
mode on an existing installation; use the host runner described in
[development checks](development.md#continuous-integration).

### Signed-packet rehearsal

The same disposable runner accepts `--signed-packets` with independently reviewed
`previous.json`, `candidate.json`, `previous.zip`, `candidate.zip`, `caddy.tar.gz`
and the pinned Linux verifier `gh.tar.gz` in a private input directory. Obtain
records from the trusted catalog/reviewed signer identities, never from downloaded
self-descriptions. Both packets must authenticate on the host before any VM is
created; every guest boot reauthenticates them with the pinned verifier. No
source-built archive or synthetic reservation is substituted in this mode.

```sh
python3 scripts/check_upgrade_reboot.py \
  --fixtures /private/signed-upgrade-inputs --signed-packets \
  --gh /trusted/gh --report /private/new-signed-upgrade-report.json \
  --boot-timeout 3600
```

The fixture runs production `install_bundle.py install` and upgrade `plan`,
`apply`, `status` and confirmed `recover` commands. A corrupt candidate must fail
at the transport-digest gate before managed state exists. The controlled
unit/receipt interruption still uses the verified engine directly for fault
injection; subsequent recovery uses the production CLI. Login, data, stopped
pending services, retries and four distinct kernel boots retain the same checks.
The report must also match both independently authenticated reservation identities
and confirm signed inputs/production CLI/corruption rejection. Missing or different
evidence fails the runner.

Only the private guest maps the fixture hostname to loopback and installs its
generated local CA into its OS trust store, so production HTTPS readiness uses
normal certificate/hostname validation. No production TLS or signature bypass is
added. Owner setup still uses a PTY with the public disposable fixture password;
no password or session is retained in the host report. macOS uses its built-in
`hdiutil` seed builder and software emulation; Linux uses xorriso and KVM when
available. The explicit boot budget can be 600–3600 seconds for signed verification
and software emulation; ordinary CI keeps its ten-minute bound. The signed guest's
fixture service has a bounded 15-minute start deadline. Temporary guest disks,
input shares and private owner/certificate/session state are removed afterward.

The [signed-packet base upgrade/reboot rehearsal](bundle-upgrade-acceptance.md)
now passed for the cataloged preview.1-to-preview.2 pair. Coordinated hosted-app
state and migration recovery still need their own acceptance. The owner
notification/SSH action flow above has rendering/authentication/fault coverage
and is checked after the real disposable-system upgrade in CI. Stable/channel promotion, canary duration/thresholds,
a promotion stop mechanism and #21 publication reconciliation remain unimplemented.
Missing evidence cannot enable promotion: metadata still rejects stable and every
packet/transaction remains `publication_eligible: false`.
