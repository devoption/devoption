# From a node to a maintained web app

Use this path for the current single-node capability: one owner, one trusted
stateless Rust/WASIp2 app and two HTTPS hostnames. Source editing and node setup
are operator actions; build review, release promotion, deployment, health and
rollback are available in the interface. There is no browser code editor,
arbitrary framework build, database or app login in this first profile.

Start with a disposable Ubuntu 24.04 x86_64 VM. A production VM is unnecessary
for development validation. Public DNS/ACME and clean cloud installation remain
pending acceptance under [#234](https://github.com/devoption/devoption/issues/234);
this guide does not mark that issue complete.

## 1. Install and sign in

Follow [platform installation](public-linux-install.md) using a reviewed commit,
the existing documented account/owner setup, pinned toolchain and Caddy. Build
with `cargo build --workspace --release --locked --offline` after dependency
acquisition, so the separate app runtime is available too. Record the exact
commit, OS image and installed versions. Verify HTTPS sign-in and create a named
project. Keep its ID from the project URL.

For local rehearsal use the [local HTTPS fixture](app-hostnames.md#local-validation)
with explicit test CA verification and isolated routing. This proves neither
public DNS nor public certificate issuance. Do not install rehearsal trust on a
production node.

## 2. Supply the app and install build/deployment support

Follow [Prepare the node](web-app-builds.md#prepare-the-node): copy the committed
public example into a standalone Git repository under the configured repository
root, prepare the pinned WASIp2 toolchain and authenticated vendor bundle, and
install the fixed build helpers and reviewed configuration. Create the selected
history/jobs directories privately, owned by the platform account, before running
the configuration utility. Keep owner data and all roots unchanged.

Then follow [deployment installation](web-app-deployments.md#operator-installation)
for the runtime, root-owned release store, deployment configuration and narrow
sudo rule. Both configurations must select the same UID, owner data, repositories,
history and build work. Do not start the app manually to bypass review.

Generate the service from those configurations instead of transcribing their paths:

```sh
python3 scripts/prepare_platform_service.py \
  --build-config /etc/devoption/build.toml \
  --deployment-config /etc/devoption/deployment.toml \
  --account devoption --binary /opt/devoption/devoption \
  --hostname platform.example.com --port 3000 \
  --output ./service-review
```

The command creates a new review directory only. It refuses mismatched storage,
UID or ingress settings, unsafe systemd text and an existing output directory.
It does not validate installed ownership, resolve account names, install files,
change sudo rules or launch anything. Check `id devoption` against both configured
UIDs and confirm its group is also `devoption`; substitute your actual account.
Paths must exist, be canonical and satisfy the brokers' ownership checks.

The generated service grants writes only to the four configured platform/build
storage roots and, when enabled, the root-owned release store. Unix ownership
still denies the unprivileged platform direct access to that store. It permits reading home directories for an existing toolchain,
and enables the fixed sudo brokers with `NoNewPrivileges=false`. Unlike the
inspection-only service, it does not clear the capability bounding set: doing so
would also prevent the root broker from performing its privileged operations.
Compiler and app workers retain their separate restricted unit policies. Review
this explicit change in the platform service's trust boundary before installation.

Retain the previous unit in a private backup location. Verify the staged unit on
the target Ubuntu host, then replace only this installation's platform unit:

```sh
sudo systemd-analyze --man=no --recursive-errors=no verify service-review/devoption-public.service
sudo systemctl stop devoption-public.service
sudo install -o root -g root -m 644 service-review/devoption-public.service /etc/systemd/system/devoption-public.service
sudo systemctl daemon-reload
sudo systemctl start devoption-public.service
sudo systemctl status devoption-public.service --no-pager
```

Sign in again. Startup validates installed brokers, storage and prior execution
state. On failure inspect bounded service logs and restore the retained unit;
do not remove owner/history/release data. To enable builds without deployment,
omit `--deployment-config`; add it later after runtime installation.

## 3. Build and deploy from the interface

1. In the project, **Code → Attach repository**, select the prepared app.
2. **Builds → Build app → Prepare build review**. Verify the committed source,
   inputs and policy, then **Approve and build**. Reload the saved attempt until
   it reaches Succeeded; inspect compiler output on failure.
3. **Releases → Save release** for that successful build. Expand the saved release,
   enter `ok A` as the exact expected health response, **Review deployment**, then
   **Approve and deploy**. Verify Healthy and the saved artifact identity.
4. Follow [app hostname setup](app-hostnames.md) using the saved project ID. The
   generated Caddyfile serves both hostnames on the same server. Retain the service
   options above. Installing the binding changes the runtime fingerprint, so sign
   in and approve a fresh deployment review afterward.
5. Use **Open app**. Verify the page and `/healthz` externally when public routing
   is available. “Local HTTPS route verified” checks the local proxy and certificate;
   it does not establish that public DNS points to this VM.

## 4. Update, fail and recover

In the trusted checkout, edit the example's revision A response to B and commit
it. Repeat build, Save release and deployment review with `ok B`. Verify the same
app URL serves B. To test failed-candidate handling on a disposable installation,
review B with an intentionally wrong health body: it must report failure, not
Healthy. Select saved A and approve `ok A` to restore its original bytes without
rebuilding. See [failure/recovery semantics](web-app-deployments.md#updates-failures-and-recovery).

Restart the platform and then reboot the disposable VM. Sign in again; verify
project, build and release records, both HTTPS URLs and the approved app's health.
Wait for actual responses: systemd “active” is not readiness. Repeat after **Stop
app** and confirm it remains stopped after reboot. No new build should start.

## Recovery boundary and acceptance record

Use the [bounded same-node snapshot and staged restore](same-node-recovery.md)
for owner/catalog/profile records, build history/jobs and saved releases together.
It preserves the working installation and requires the original repository and
prepared-input roots, binaries/configuration and Caddy state to remain available.
It is not a portable machine backup. Combined removal remains unperformed; never assume an owner-record copy restores an app.

For a data-preserving removal, first cancel any build and wait for its terminal
state, then **Stop app** and confirm it stopped. Disable/stop this installation's
platform and Caddy units. Have the administrator verify the fixed build/app units
are inactive before removing only inspected installed units, helper/binary files
and their dedicated sudo entries. Preserve all state, repositories, certificates,
inputs and backups; do not delete accounts automatically. This combined removal
still needs rehearsal; the platform-only uninstall evidence is narrower.

Record each row as passed, failed or unperformed with exact revision and evidence:

| Acceptance step | Existing evidence / remaining work |
| --- | --- |
| Install and HTTPS login | [Local Ubuntu service/reboot rehearsal](public-linux-rehearsal.md); clean cloud install pending |
| Create project and build A | [Real local interface builds](web-app-builds.md#local-evidence-and-remaining-limits) |
| Save/deploy, failed candidate, return to A | [Local Wasmtime/systemd rehearsal](web-app-deployments.md#disposable-local-evidence) |
| Two hostnames, A → B → A, reboot and stopped intent | [Local HTTPS rehearsal](app-hostnames.md#observed-ubuntu-rehearsal) |
| Public DNS, ACME issuance/renewal, external access | Unperformed; requires separately supplied host/DNS authority |
| Bounded same-node record snapshot and restore | [Workflow and validation scope](same-node-recovery.md); same-node recovery passed; combined removal unperformed |

The linked rehearsals were separate passes on an existing disposable VM, not one
fresh-machine acceptance run. Keep that distinction in the final evidence. Product
release approval remains separate under #21.

## Service generator validation

The generated unit passed `systemd-analyze verify` on the existing disposable
Ubuntu 24.04 x86_64 VM. With the already installed build/deployment brokers and
saved fixture state, it completed build-unit reconciliation, deployment status
and stopped-intent reconciliation, then served the expected 303 sign-in redirect.
The app worker stayed inactive. The temporary platform unit was then stopped and
removed. No new build or app deployment was started by this check. This validates
service startup and broker access, not a fresh installation or a new full browser
build/deploy run under this unit.
