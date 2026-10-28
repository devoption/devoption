# Independent hosted apps on one node

The opt-in deployment broker supports one to eight explicitly configured project
slots. Each slot binds one saved project to one HTTPS hostname, loopback listener,
release store, approved intent, health observation, log directory and pair of
systemd units. An authenticated owner builds, promotes and approves releases
through that project's existing interface. Adding a project alone does not create
a slot. This remains trusted-app hosting with one platform owner.

The existing single-app `port`, `[ingress]`, `[records]` and `[https]` configuration
remains supported. Multi-app mode replaces those top-level fields with `[[apps]]`;
mixing the two schemas is rejected. No browser action provisions slots, changes
DNS, grants credentials or selects a unit/path/port.

## Operator configuration

Install the [build and deployment support](web-app-deployments.md) first. Create
both projects and attach their checkouts before enabling their slots. Use actual
32-character lowercase hexadecimal project IDs from their URLs. For example:

```toml
uid = 1000
build_work = "/home/devoption/build-jobs"
state = "/var/lib/devoption-apps"
worker = "/usr/local/libexec/devoption-app-worker"
owner_data = "/home/devoption/owner-data"
history = "/home/devoption/build-history"
repositories = "/home/devoption/repositories"

[[apps]]
project = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
port = 15000
[apps.ingress]
platform_hostname = "platform.example.com"
platform_port = 3000
app_hostname = "first.example.com"
project = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[apps]]
project = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
port = 15001
[apps.ingress]
platform_hostname = "platform.example.com"
platform_port = 3000
app_hostname = "second.example.com"
project = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
```

Create the root and each `state/PROJECT_ID` directory as root-owned mode 0700.
Existing protected-path, worker, storage-disjointness and non-root-caller rules
apply. The broker never creates these directories. Every app must have an ingress
binding, with the same platform origin/port. Duplicate projects, hostnames or
listeners, a platform/app collision, unknown slot fields and ambiguous bindings
are rejected before deployment. All configured projects must exist in the catalog.

Optional `[apps.records]` and `[apps.https]` tables use the existing
[record grant](app-records.md) and [HTTPS policy](app-https.md) schemas. Each grant
must match its slot's project. Record-enabled apps require distinct non-login
service UIDs. Secret revisions remain bound to individual releases and reviewed
deployments; secret values remain outside configuration and release metadata.
No database-access or application-authentication capability is added here.

Stage the complete proxy and unchanged deployment configuration together:

```sh
python3 scripts/configure_app_hostname.py \
  --deployment-config ./deployment.toml --all-apps \
  --platform-hostname platform.example.com --platform-port 3000 \
  --platform-access-policy ./platform-access.toml \
  --output ./multi-app-review
```

Use the existing administrative client-CA and permitted-leaf policy. An explicit
`--public-platform` choice is available for installations using ordinary owner
authentication. Each app gets the same independent cookie/header stripping and
reserved-control-path rules; client-certificate requirements apply only to the
platform. Review and validate the staged Caddyfile before the operator installs
the pair using the [hostname installation procedure](app-hostnames.md).
Point each app's DNS at the node and keep all backend ports private. Neither
generation nor deployment changes DNS or proves public certificate issuance.
`prepare_platform_service.py` checks every slot against the selected origin/port.

## Migrating a single-app installation

Back up the current configuration and deployment state. Stop the legacy app
through the existing authenticated interface and confirm its unit is inactive
with an empty cgroup. Stop the platform before replacing its broker/worker and
configuration. Never rebind a live legacy listener to a new slot.

Select a **new empty state root**, leaving the old store preserved outside it.
Create the project directories there. To reuse saved releases, inspect each old
manifest's project and copy only its complete immutable release directory into
the matching slot, preserving root ownership and permissions. Do not copy
`desired.json`, `last-action.json`, pending writes or lock files: the migrated app
requires a new explicit deployment review. Otherwise rebuild and promote through
the project interface. Never edit manifests or invent missing provenance.

The broker refuses multi-app mode when the selected root contains legacy releases
or intent. This avoids silently abandoning an installation. Review the new proxy
and platform service configuration, install them, restart the platform, then
approve each project's destination and saved release separately. A mode change
does not authorize resuming the former running intent. Retain the original backup
until independently checking both applications and recovery.

## Independent lifecycle and bounds

Releases shows the selected project's status and destination before approval.
An unconfigured project cannot promote, start, stop or reconcile another slot.
Saved releases and previous healthy selections live in the project's directory;
the existing 32-release bound applies independently to each slot. Pending browser
reviews are project-scoped, single-use and lost on restart. Changes to one slot's
hostname, port or grants invalidate its runtime fingerprint without invalidating
another slot. Changes to shared broker/worker bytes affect every slot and require
the existing fresh-review/recovery policy.

Units are derived only from configured IDs:
`devoption-app-PROJECT_ID.service` and
`devoption-app-PROJECT_ID-validate.service`. Logs use
`/run/devoption-app-PROJECT_ID/worker.log`; the host selects this directory via
systemd's `RUNTIME_DIRECTORY`, which is never inherited by the guest. Each worker
retains the [existing resource and capability limits](web-app-deployments.md#enforced-runtime-policy).
These are per-app limits, not an installation-wide scheduler: size the node for
the aggregate memory/CPU allowance. Broker mutations remain serialized by the
installation lock; compilation validation or an update may delay control actions
but does not stop another slot's worker.

Updates, failed candidates, explicit saved-release rollback and stopping target
only the selected slot's units and intent. On startup the controller checks each
saved release's project/repository association and independently reconciles its
approved intent and selected secrets. Stopped intent stays stopped. Health checks
require that slot's current systemd invocation. A healthy service or a screenshot
alone does not establish public reachability. Adding/removing slots and changing
proxy configuration remain operator maintenance; stop a slot before removing it.
There is no automatic orphan-unit deletion or product upgrade expansion.

## Validation

Run the [normal checks](development.md#checks). `scripts/test_multi_app.py` covers
strict configuration, independent fingerprints, wrong-project refusal and
update/rollback/stop/recovery isolation. Existing singleton, capability and
protected-proxy tests remain applicable.

The disposable Linux CI checker `scripts/check_multi_app.py` uses two real Git
checkouts, owner-authenticated build/release/deployment endpoints, real offline
builds, systemd workers and protected platform/app HTTPS routes with a private
test CA. It checks updates, saved rollback, stopping and platform-service restart.
It refuses existing installations and removes only its fixture files/units.

Its explicit temporary export feeds `scripts/check_multi_app_reboot.py`, which
boots an isolated Ubuntu guest three times, checks both running deployments,
recovery and independent stopped/running intent with local verified HTTPS.
The guest reconciles the exported approved intent through the production broker;
it does not rebuild apps or repeat the controller workflow. Empty placeholders for
the original temporary input directories and public proxy verification files are
recreated on each boot; only the exported deployment and proxy state are recovered. The export contains
only disposable runtime/proxy state and test-CA material, never the owner store,
source, compiler inputs or production credentials. CI does not upload it.
Reports require three distinct kernel boot IDs and all phases; failed/missing
evidence cannot pass. These checks establish local TLS and lifecycle behavior,
not public DNS/ACME, arbitrary workloads or hostile multi-tenant isolation.
