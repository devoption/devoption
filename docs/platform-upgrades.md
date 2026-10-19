# Operator-triggered preview upgrades

The first upgrade path under [#263](https://github.com/devoption/devoption/issues/263)
is a **same-schema base bundle installation** on dedicated Ubuntu 24.04 amd64.
`scripts/upgrade_bundle.py` authenticates both packets, records a bounded offline
recovery point, activates the candidate and checks private/public readiness.
It has no publication or unattended-upgrade operation. Operator approval of a PR
is not approval to run it on an existing server.

The currently retained `0.1.0-preview.1` packet remains fresh-only. There is no
signed upgrade target yet. CI builds a real previous-source executable and a
separate next-preview executable as disposable fixtures; those are not release
artifacts or permission to sign/publish a new packet.

## Supported compatibility boundary

A candidate must use [release metadata schema 2](release-metadata.md), declare
`installation = "fresh-and-upgrade"`, and list **exactly the preceding numbered
preview of the same base version** in `upgrade_from`. Both `data_schema` and
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

No matching signed upgrade target currently exists, so these are the invocation
contract, not a runnable upgrade of the retained preview today. Do not substitute
unsigned CI fixture inputs into the production CLI. `--allow-validation-candidate`
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
accounts, units and state afterward. Ordinary CI permissions remain read-only;
there are no production credentials, self-hosted runners, signing or publication
steps. See [development checks](development.md#continuous-integration).

This is base-upgrade implementation and fixture evidence. Real signed
previous-to-candidate acceptance, reboot recovery of pending/completed transactions,
coordinated hosted-app state, migration recovery and notification/UI actions still
need their own acceptance. Stable/channel promotion, canary duration/thresholds,
a promotion stop mechanism and #21 publication reconciliation remain unimplemented.
Missing evidence cannot enable promotion: metadata still rejects stable and every
packet/transaction remains `publication_eligible: false`.
