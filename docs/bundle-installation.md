# Verified Ubuntu bundle installation

The installer under [#262](https://github.com/devoption/devoption/issues/262)
installs a reviewed signed preview packet without compiling DevOption. It reuses
[the public installation](public-linux-install.md)'s dedicated accounts, hardened
systemd units, private backend and pinned Caddy configuration. Build/deployment
capabilities remain the separate opt-ins described in that guide.
For those opt-ins, use `/opt/devoption/releases/VERSION/bin/devoption` wherever
the source-install guide selects `/opt/devoption/devoption`; the installer creates
no mutable binary alias.

This is a **fresh-host validation path**, not a published or recommended release
channel. The current retained packet is an unpublished validation candidate;
`--allow-validation-candidate` is mandatory. It does not authorize publication,
install an upgrade or change release eligibility. Do not use it to overwrite a
source installation. The installer itself runs from a separately reviewed trusted
checkout, not code extracted from the downloaded packet.

## Prepare the dedicated host and inputs

Use Ubuntu 24.04 amd64, systemd, cgroup v2, existing administrator/SSH access and
a dedicated node without untrusted shell users. Required Ubuntu packages are
Python 3.11+, Git and sudo (from Ubuntu's signed archives); `useradd`, `runuser`,
`getent`, `systemctl` and `systemd-analyze` must be available. DevOption needs no
Rust toolchain or C compiler for installation. Later app builds need the separate
[pinned toolchain/vendor preparation](web-app-builds.md).

Use the [pinned GitHub CLI 2.102.0 verifier and trust policy](bundle-provenance.md).
Install that trusted verifier before running the installer as root; do not use a
binary supplied by the candidate as the verifier. Review the installer commit,
its CI and the reservation JSON independently. A checksum or reservation downloaded
from the same untrusted location as the packet is not an independent trust anchor.

The host must have free TCP ports 80, 443 and 3000, and at least **1 GiB plus twice
the unpacked payload size** free on each of `/opt`, `/var/lib` and `/etc` for the
conservative install/staging budget. This is an installer disk check, not an app
build capacity promise. The installer refuses existing `devoption`/`caddy`
accounts/groups, platform/proxy directories or units without its own matching
receipt. It does not stop unrelated listeners or remove conflicting installations.

Follow [DNS, firewall and ACME prerequisites](public-linux-install.md#supported-shape-and-prerequisites)
first: one lowercase DNS hostname, inbound 80/443 to Caddy, SSH restricted to your
administration path, no public 3000, and outbound DNS/HTTPS. DNS/firewall changes,
paid hosts and privileged installation require operator authorization separately
from preparing the PR.

In an unprivileged administrator session, obtain a trusted installer checkout
and select its full reviewed commit. No Cargo build is needed:

```sh
git clone https://github.com/devoption/devoption.git
cd devoption
git checkout --detach REVIEWED_INSTALLER_COMMIT
```

Obtain the exact reserved candidate ZIP. For the current validation candidate,
its retained asset ID is `656905392`; access to the private draft needs your
existing GitHub authentication. Download **before** sudo, without passing tokens
in command arguments:

```sh
gh api --hostname github.com --method GET \
  repos/devoption/devoption/releases/assets/656905392 \
  -H 'Accept: application/octet-stream' > /tmp/devoption-candidate.zip
```

Do not unpack or execute this ZIP manually. The independently reviewed reservation
is `release-candidates/0.1.0-preview.1.json` from the trusted checkout. The original
bundle's embedded documentation/metadata describes its assembly-time limits; this
trusted installer was added afterward and does not rewrite that immutable bundle.

Download the Caddy archive using the exact URL/checksum acquisition documented
in [public installation](public-linux-install.md#install-the-pinned-proxy-and-configure-tls).
Keep the archive as `/tmp/caddy.tar.gz`; do not install it manually first. The
installer independently checks its pinned SHA-512 against both its trusted code
and authenticated bundle manifest, then copies only its bounded `caddy` member.
It never installs OS packages, downloads toolchains, changes DNS/firewalls or
creates GitHub credentials.

## Plan, install, enroll, start

Run from that trusted checkout. Replace the hostname and verifier path below.
These examples use Bash arrays so each argument remains separate:

```bash
installer_args=(
  --reservation "$PWD/release-candidates/0.1.0-preview.1.json"
  --packet /tmp/devoption-candidate.zip
  --gh /usr/local/bin/gh
  --caddy-archive /tmp/caddy.tar.gz
  --hostname platform.example.com
  --allow-validation-candidate
)
sudo python3 scripts/install_bundle.py plan "${installer_args[@]}"
sudo python3 scripts/install_bundle.py install "${installer_args[@]}"
```

`plan` performs signature/digest/manifest/host checks and reports existing state
and missing files without installing resources. All modes reverify private input
snapshots. A wrong target, signature, digest, manifest, host, existing installation
or acquisition pin stops before account/service/file installation changes.
The optional `--asset-id ID` replaces `--packet FILE` and downloads only from the
fixed DevOption release-asset API; the result must still match the independently
reviewed reservation. Prefer the predownloaded file for a draft so root needs no
GitHub authentication.

`install` creates two non-login service accounts, versioned read-only files under
`/opt/devoption/releases/VERSION`, a pinned proxy under `/opt/devoption/proxy`,
units under `/etc/systemd/system` and `/etc/caddy/Caddyfile`. Persistent platform
state stays in `/var/lib/devoption/{data,repositories}`; Caddy state stays in
`/var/lib/caddy/{data,config}`. The root-only installation identity is recorded
in `/var/lib/devoption-installer/installation.json`. It validates the generated
units/proxy configuration and reloads systemd; it does **not** enroll an owner or
start services. No preview setup page is exposed while owner setup is pending.

From an interactive SSH terminal (`ssh -t`), run:

```bash
sudo python3 scripts/install_bundle.py enroll "${installer_args[@]}"
sudo python3 scripts/install_bundle.py start "${installer_args[@]}"
sudo python3 scripts/install_bundle.py status "${installer_args[@]}"
sudo systemctl status devoption-public.service caddy-devoption.service --no-pager
curl --fail --head https://platform.example.com/projects
```

Enrollment invokes the verified installed executable as the unprivileged
`devoption` account and prompts through the terminal. Never put the owner password
in arguments, environment variables, automation or logs. An existing nonempty
private owner record is preserved; a missing/corrupt record is not deleted as a
retry strategy. Runtime still validates its actual record format.

`start` requires completed installation and an owner record, validates configuration,
enables both services for boot and waits for the backend's unauthenticated `303`
redirect to `/sign-in`. **That local check does not prove public TLS readiness.**
The external curl should return a trusted HTTPS response and `303`. Verify browser
login, project persistence and sign-out, then reboot the disposable host and check
both services/HTTPS again. Do not bypass certificate verification on a real host.

Stop with `sudo systemctl stop devoption-public.service caddy-devoption.service`.
Restart with `sudo systemctl restart devoption-public.service caddy-devoption.service`,
then repeat readiness/browser checks. Use `journalctl -u devoption-public.service
-u caddy-devoption.service -n 50 --no-pager` for bounded diagnostics. Existing
[proxy troubleshooting](public-linux-install.md#failure-diagnosis-and-restart-verification)
and [offline backup/recovery](same-node-recovery.md) rules still apply.
Record the installed versioned binary path from the unit, the reviewed reservation
and the installer receipt with the backup; substitute that binary path in examples
written for a source installation. Preserve proxy private keys separately.

## Reruns, interruptions and upgrades

Repeat the same `install` arguments to resume committed steps. The installer
reconciles actual resources rather than executing a saved command list. Completed
files must match their verified bytes, ownership and mode, and are never replaced;
owner, project/profile, repository and Caddy data are never overwritten. Repeated
enrollment preserves the existing owner. A host-wide lock serializes mutations.
Version or hostname changes are refused: use the future platform upgrade/configuration
contract under #263 rather than treating reinstall as upgrade.

A completed atomic file commit can be resumed after interruption. Conflicting
accounts/groups, corrupted files, changed units, symlinks, hard links or private
state with no valid receipt stop for manual inspection. A crash in the narrow
initial receipt reservation or temporary-hard-link cleanup window can also require
inspection; the installer will not guess ownership or delete unknown partial
state. Preserve the error and completed files, restore the matching inputs and
inspect before retrying. No automatic rollback deletes persistent data or stops
an unrelated installation.

The same Python installer/host boundary is intended for future image preparation
under #264. No separate image installer or Marketplace launch is implemented here.

## Validation scope

Python fixtures cover verified-packet policy, signature/digest failure, proxy pin
failure, download failure, host/port/conflict rejection, interrupted file commits,
changed identities/bytes/modes/links, interactive enrollment requirements and
state-preserving reruns. Fixture cryptographic success is mocked and is not a
new signed-release acceptance claim.

Ubuntu CI additionally installs its trusted locally assembled release executable
into disposable paths through the production file-placement engine, verifies
rendered units using systemd's parser, performs real PTY owner setup, checks
loopback readiness, restarts that exact binary and repeats installation while
preserving owner bytes. It creates no system accounts, installs no host units and
changes no system services. **Real root installation, public DNS/ACME and reboot
acceptance of this installer remain pending on an explicitly authorized disposable
host.** Prior source-install rehearsals do not establish those results for this path.
