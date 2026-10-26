# Back up and stage a same-node recovery

This workflow protects the current owner/catalog/profile records, build history,
retained job artifacts/receipts and saved app releases together. It is a bounded
**same-installation** snapshot, not a portable machine backup. It requires the
original repository objects, prepared toolchain/vendor trees, installed binaries,
helpers, configuration and Caddy state to remain available. It never installs a
restore or starts an app.

Use it before operator maintenance on a trusted single-node installation. All
writers must remain stopped throughout creation; the tool checks the named system
unit and three fixed worker units, holds existing writer locks, compares source
and copied inventories, and rechecks unit state. It cannot discover an unrelated
foreground process or prevent a trusted administrator from changing files. Stop
those writers too. Do not use it for hostile concurrent writers or network filesystems.

The optional [project secret store](project-secrets.md) is included as encrypted
owner-directory records. Its external installation key is not copied; retain a
separately protected matching key backup and original owner identity.

## Create and verify

Cancel any active build in the interface and wait for a terminal result. Use
**Stop app** and confirm it stopped. Stop the platform's actual system unit.
Do not merely kill the worker: saved running intent deliberately blocks backup.
The command does not stop services for you.

From the reviewed source checkout, substitute the actual unit, binary and full
source commit corresponding to the installed binary:

```sh
sudo systemctl stop devoption-public.service
sudo python3 scripts/recovery_snapshot.py create \
  --build-config /etc/devoption/build.toml \
  --deployment-config /etc/devoption/deployment.toml \
  --binary /opt/devoption/devoption \
  --platform-unit devoption-public.service \
  --source-commit FULL_40_CHARACTER_REVIEWED_COMMIT \
  --output /var/backups/devoption-before-maintenance
sudo python3 scripts/recovery_snapshot.py verify \
  --snapshot /var/backups/devoption-before-maintenance
```

The destination must not exist. The default content budget is 64 MiB, with at most
20,000 entries. An explicit `--max-bytes` may raise it up to 268435456 (256 MiB).
This bounds copied data, not a disk reservation or execution deadline. Check free
space first; the original, snapshot and staged restore coexist. A failed operation
leaves evidence for inspection and never adopts a partial restore. Choose a new
destination for retry instead of deleting working data.

The private snapshot contains `payload/{owner,history,jobs,releases}` and a
manifest recording SHA-256, sizes, numeric ownership/modes, original paths and
source revision. It excludes the four original writer locks and the optional `.project-secrets.lock`; it rejects links, special
files, special permission bits and pending temporary state. Storage roots must
retain their private installed ownership/mode; ordinary child modes are preserved. Missing optional records remain absent. No browser session or approval is
restored. Treat the snapshot and manifest as private: owner hashes, source, build
logs and absolute paths are sensitive. SHA-256 detects corruption; it is not an
authenticated signature against someone who can replace the snapshot.

The manifest also records hashes of installed binaries, helpers, configuration
and optional rehearsal CA, plus identities of the original repository/toolchain/
vendor root directories. These are **references**, not copied files. It does not
hash every external tree entry; normal platform startup and build review still
perform their existing identity/input verification. The operator-supplied source
commit is a receipt, not a cryptographic attestation of binary provenance.

After a verified backup, start the original platform if continuing normal work.
The app remains stopped until a new explicit deployment review. Never roll back
build history to hide later attempts or regain capacity: this procedure is for
recovery of a stopped installation, not history retention.

## Stage without overwriting

Before changing the original installation, stage the snapshot into a new directory:

```sh
sudo python3 scripts/recovery_snapshot.py stage \
  --snapshot /var/backups/devoption-before-maintenance \
  --output /var/lib/devoption-restored
```

Staging first verifies the entire payload and its metadata, then checks the
referenced installation has not changed. It refuses a changed binary/configuration,
changed external root identity, corrupt snapshot, existing destination or any
destination overlapping the snapshot/original data. For later adoption, choose a destination whose ancestors are root-owned and not
group/other writable, such as the `/var/lib` example; the release broker rejects
state beneath a user-owned home directory. It preserves numeric UID/GID
and modes and verifies the resulting copy. It does not rewrite the active configs,
service unit or source repositories. Restore from the same reviewed source/tool
version; a changed installation requires operator investigation, not bypassing the
reference checks. No cross-version migration is promised.

## Validate and select the staged state

Keep the original installation and snapshot intact. Stop all relevant services
before switching. Review these replacements in **copies** of both TOML configs:

| Configuration field | Staged value |
| --- | --- |
| Both: `owner_data` | `/var/lib/devoption-restored/owner` |
| Both: `history` | `/var/lib/devoption-restored/history` |
| Build: `work`; deployment: `build_work` | `/var/lib/devoption-restored/jobs` |
| Deployment: `state` | `/var/lib/devoption-restored/releases` |

Keep the UID, repository root, toolchain/vendor paths, runtime binary, ingress
binding and all other fields unchanged. The root-owned staging parent starts at
0700; allow the existing platform account to open each ancestor directory with
`sudo chmod 755 /var/lib/devoption-restored`. The store opens ancestors for reading,
so execute-only traversal (0711) is insufficient. Its owner/history/jobs children retain
private platform ownership; releases stays root-owned 0700. Do not make the release
store writable by the platform account.

Generate and verify the [platform service](first-web-app.md#2-supply-the-app-and-install-builddeployment-support)
from the staged configs. Retain the original config and unit files, then install
the reviewed staged configs at the fixed `/etc/devoption` paths and the generated
unit at this installation's existing service path. Start only that platform unit.
The changed configuration fingerprint invalidates old deployment reviews. Stopped
intent remains stopped; no app or build should start automatically.

Sign in with the existing owner password. Check project/repository names, saved
build logs/results and saved release IDs, artifact hashes and sizes. Verify old
sessions fail after restart. In a disposable rehearsal, approve a **fresh** review
of a saved release to test actual serving, then stop it again. Returning to the
original installation requires stopping the restored platform and reinstalling
its retained original configs/unit; keep both state trees for inspection.

If startup, login or identity checks fail, stop and investigate. Do not rerun
owner setup, erase history, edit release manifests or silently reattach repositories.
The existing device-binding recovery procedure applies only to its documented
narrow case; copied repositories on a new disk are not proven by this workflow.

## Limits and remaining acceptance

Caddy certificates/configuration, repository contents, toolchains and executable
files need separate storage protection. Only file contents, numeric owners and ordinary mode bits are copied; timestamps,
ACLs and extended attributes are not preserved. This snapshot alone cannot recover
loss of the machine or disk. Off-host encrypted backups, retention, portable repository
recovery and full-node disaster recovery are outside this tool. The fresh cloud
and public DNS/ACME acceptance under #234 remains separate.

## Observed local recovery

On the existing disposable Ubuntu 24.04 x86_64 VM, a roughly 2 MiB snapshot of
real owner/project state, successful build records/artifacts and three saved Wasm
releases verified and staged into new root-protected storage. The compatible
installed platform was built from commit
`5c33bf6d186e43508ec238ed9b3fd0c1101d11e0`; no platform rebuild or app compilation
was needed for recovery.

Using temporarily installed configs and the generated service, the restored owner
signed in through the authenticated local HTTP diagnostic path. Project, Builds
and Releases pages loaded, including the original saved A release identity.
The build and app units initially remained inactive. A fresh authenticated
deployment review then started restored saved A; its private `/healthz` returned
`ok A` without a rebuild. Stop app succeeded, and a subsequent platform restart
kept it stopped and rejected the previous session. The test then restored the original configs byte-for-byte, stopped and
removed its temporary service, and retained original/snapshot/restored data. Early
checks exposed insufficient ancestor read permission and an unsuitable user-owned
ancestor for release storage; the instructions above use the validated protected
path and permissions.

This verifies record/artifact recovery and service reconciliation on the same
installation, including serving a restored release through the existing
authenticated deployment API. It did not change Caddy/DNS, exercise public HTTPS
or simulate disk loss. Existing app-serving/reboot evidence remains in the
[deployment guide](web-app-deployments.md#disposable-local-evidence). Automated
snapshot tests also cover corruption, missing/extra files, links, busy locks,
running intent, size bounds, changed references and source changes during copy.

The later [clean local rehearsal](clean-local-rehearsal.md) repeated snapshot
adoption through the HTTPS browser interface, served saved A at the app hostname,
and verified combined data-preserving removal/reinstallation. Its prepared-binary
and public DNS/ACME limitations remain explicit.
