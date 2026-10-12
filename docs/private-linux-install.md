# Private Ubuntu source installation

This guide covers the current runtime: CLI owner setup, browser sign-in, page
access, read-only saved-project lists, and browser-local appearance settings.
Project registration, builds, artifacts and deployments remain unimplemented. It is one step under
[#20](https://github.com/devoption/devoption/issues/20), tracked by
[#90](https://github.com/devoption/devoption/issues/90), not a complete software
factory installation or a published release.

The target is Ubuntu 24.04 x86_64 with an existing unprivileged login account,
Git, Rust 1.99.0 via rustup, a C toolchain/linker, and a systemd user manager.
For remote use, SSH access and local forwarding must already work. This guide
does not create OS accounts, install system packages or configure SSH. The basic
user-service setup does not enable lingering; an optional, separately approved
step is described below. An administrator must provision missing prerequisites.
See the [Rust installation guide](https://www.rust-lang.org/tools/install).

## Build and prepare private storage

Run as the account that will run DevOption, on the target host. For a remote
terminal, use `ssh -t user@server` with your existing SSH access. Initial Cargo
fetching needs network access; subsequent commands use the lockfile offline.

```sh
git clone https://github.com/devoption/devoption.git
cd devoption
git rev-parse HEAD
cargo fetch --locked
cargo build --release --locked --offline
```

Record the source commit. Use a reviewed commit with passing CI; `main` can move.
The following fresh-install block checks the physical home path, all existing
parent directories, and both destinations before creating directories. It rejects
symlink components, non-directory parents, and existing destinations, including
dangling symlinks. Keep existing installations and data; do not rerun setup over
them.
The home directory and all data-path components must be real directories without
symlinks or `..`. If your home layout differs, choose an absolute real path and
adjust both commands and the service example consistently.

```sh
(
  set -eu
  umask 077
  test "$(cd "$HOME" && pwd -P)" = "$HOME"
  for ancestor in "$HOME/.local" "$HOME/.local/lib" "$HOME/.local/share"; do
    test ! -L "$ancestor"
    if [ -e "$ancestor" ]; then
      test -d "$ancestor"
    fi
  done
  for destination in "$HOME/.local/lib/devoption" "$HOME/.local/share/devoption"; do
    test ! -e "$destination"
    test ! -L "$destination"
  done
  mkdir -p "$HOME/.local/lib" "$HOME/.local/share"
  mkdir -m 700 "$HOME/.local/lib/devoption"
  mkdir -m 700 "$HOME/.local/share/devoption"
  install -m 700 target/release/devoption "$HOME/.local/lib/devoption/devoption"
  git rev-parse HEAD > "$HOME/.local/lib/devoption/source-commit.txt"
)
```

Run setup in the terminal, then wait for `Owner created.`:

```sh
"$HOME/.local/lib/devoption/devoption" setup --data-dir "$HOME/.local/share/devoption"
```

Enter and confirm a unique passphrase of at least 15 characters. Input is hidden;
passwords cannot be supplied in arguments, environment variables, pipes or files.
Setup creates `owner.bin`, containing the owner identity and salted Argon2id hash,
with mode `0600` inside the `0700` directory. Both must belong to this OS user.
There is no password reset command. After an interrupted or uncertain setup,
inspect the existing state; never delete a record just to retry. Read the
[setup behavior and limits](local-enrollment.md) before creating a real owner.

The data path is explicit, outside the checkout and binary directory. It remains
the same after a rebuild. The server reads the owner record and, when present,
the optional [project catalog](project-catalog.md) without creating or editing it.
Sessions and CSRF tokens are memory-only and disappear on restart; appearance
preferences stay in that browser's local storage and are not in a server backup.

## Start the user service

The [example unit](../examples/systemd/devoption.service) runs as the logged-in
account with port `3000` and the paths above. It requires a nonempty owner file
before launching; the runtime then validates its ownership, permissions and
format. This avoids starting the public, unconfigured preview after a missing
owner record. Do not remove the preflight or `--data-dir` for an owner installation.

From the repository checkout, install only this user unit:

```sh
(
  set -eu
  mkdir -p "$HOME/.config/systemd/user"
  test ! -e "$HOME/.config/systemd/user/devoption.service"
  test ! -L "$HOME/.config/systemd/user/devoption.service"
  install -m 600 examples/systemd/devoption.service "$HOME/.config/systemd/user/devoption.service"
  systemd-analyze --user --man=no --recursive-errors=no verify "$HOME/.config/systemd/user/devoption.service"
  systemctl --user daemon-reload
  systemctl --user enable --now devoption.service
)
systemctl --user status devoption.service
journalctl --user -u devoption.service -n 30 --no-pager
curl -I http://127.0.0.1:3000/
```

Expect a redirect to `/sign-in` (`303`) before authentication. A successful
`systemctl start` alone is not an HTTP readiness check. `Type=exec` checks that
the executable starts; it does not wait for the listener. Failures restart after
five seconds, subject to systemd's start limits. A deliberate stop stays stopped.
See the upstream [service semantics](https://github.com/systemd/systemd/blob/v255/man/systemd.service.xml).

`systemctl --user enable --now` enables the unit for future user-manager starts
and starts it immediately. Plain `enable` does not start the service; see
[systemctl's enable semantics](https://github.com/systemd/systemd/blob/v255/man/systemctl.xml).
Operation after logout or before the next login depends on the host's existing
user-manager policy; enabling a unit does not enable lingering.
In the [Ubuntu rehearsal](linux-rehearsal.md), the service stopped after the last
logout with `Linger=no` and started on the next login, including after reboot.
This does not promise unattended persistence on another host. If `systemctl --user`
cannot connect, stop and have the account/session prerequisites checked; do not
substitute a root service.

## Optional startup before login

If unattended startup is required, first obtain approval from the account owner
and the host administrator for account-level lingering. It starts the user's
manager at boot and keeps it after logout, affecting all enabled user services
for that account. It does not grant DevOption root privileges or change its
loopback binding. See [loginctl](https://github.com/systemd/systemd/blob/v255/man/loginctl.xml).

After owner setup and user-unit enablement, record the existing policy before
applying an approved change. Run as the account that owns this installation:

```sh
account=$(id -un)
loginctl show-user "$account" -p Linger
sudo loginctl enable-linger "$account"
loginctl show-user "$account" -p Linger
```

Expect `Linger=yes`. Verify the application is ready before the first login after
reboot; an SSH login followed by a readiness check cannot establish this on its
own. The [bounded Ubuntu rehearsal](linux-unattended-rehearsal.md) used a temporary
unprivileged probe and compared its evidence with the first login timestamp.
It also checked continued operation after logout and explicit stop behavior.

To reverse a change whose recorded baseline was `Linger=no`, use
`sudo loginctl disable-linger "$account"` and confirm `Linger=no`. Do not disable
pre-existing lingering as part of another installation's cleanup. Disabling it
does not replace `systemctl --user stop devoption.service` when an immediate
application stop is required. The rehearsal restored its original policy and
stopped the guest; it did not leave unattended operation enabled.

## Open the browser privately

On the same host, open `http://127.0.0.1:3000/`. From another computer, keep this
command running there, replacing `user@server` with your existing SSH destination:

```sh
ssh -N -o ExitOnForwardFailure=yes -L 127.0.0.1:3000:127.0.0.1:3000 user@server
```

Then open `http://127.0.0.1:3000/` on that computer and sign in. Keep **the same
port at both ends**: the browser's Host and Origin must match DevOption's actual
listening port. If `3000` is busy on either computer, pick one free unprivileged
port, change the service's `--port`, reload/restart it, and use that same port in
both forward positions and the browser URL. Do not stop unrelated listeners.

The explicit local address keeps the browser end on loopback; the server also
binds only to loopback. Do not use wildcard bindings, expose the port, weaken
Host/Origin checks, or add a public reverse proxy. SSH forwards the connection
without changing its HTTP headers. See [OpenSSH forwarding](https://man.openbsd.org/ssh.1#L).
Secure cookies have been checked with Chrome on loopback, including an SSH tunnel
to a disposable Ubuntu guest. Other browsers and public deployment remain
unvalidated. JavaScript is required for sign-in. Closing the tunnel ends access,
not the server process.

## Restart, backup and restore

Restart with `systemctl --user restart devoption.service`, then check status,
logs and the unauthenticated `303` response above. Sign in again afterward;
restart invalidates all old sessions. For a binary update, build the reviewed
revision first, stop the service, retain the previous binary and commit ID, then
replace the binary and restart. Record the new commit with it. No automatic
upgrade or storage migration is implemented.

The owner-only backup/restore examples below apply to installations without
`projects.bin`. They do not preserve optional catalog metadata; complete catalog
backup/restore has not been validated by this guide. Keep any existing catalog
and original data untouched.

For an owner-only backup, stop the service and copy into a new private directory. Run this on
the server; it leaves the original data in place and prints the backup location:

```sh
(
  set -eu
  umask 077
  systemctl --user stop devoption.service
  backup=$(mktemp -d "$HOME/devoption-backup.XXXXXXXX")
  cp -p "$HOME/.local/share/devoption/owner.bin" "$backup/owner.bin"
  cp -p "$HOME/.local/lib/devoption/devoption" "$backup/devoption"
  cp -p "$HOME/.local/lib/devoption/source-commit.txt" "$backup/source-commit.txt"
  cmp "$HOME/.local/share/devoption/owner.bin" "$backup/owner.bin"
  printf 'Backup saved in %s\n' "$backup"
  systemctl --user start devoption.service
)
```

If any step fails, the block stops; check the service state before continuing.
The owner record and its copies are sensitive password-hash material: keep them
private and out of source control, logs and issue attachments. This local copy
does not protect against loss of the host or disk. Off-host encrypted backups and
their retention policy are separate operator work. Temporary interrupted-store
files are not owner records and are not needed for this current-state backup.

To check a restore, retain the original data and select a **fresh** directory.
Replace the backup path below with the directory printed above:

```sh
(
  set -eu
  umask 077
  backup=/absolute/path/to/devoption-backup.XXXXXXXX
  systemctl --user stop devoption.service
  mkdir -m 700 "$HOME/.local/share/devoption-restored"
  cp -p "$backup/owner.bin" "$HOME/.local/share/devoption-restored/owner.bin"
  cmp "$backup/owner.bin" "$HOME/.local/share/devoption-restored/owner.bin"
  "$backup/devoption" serve --port 3000 --data-dir "$HOME/.local/share/devoption-restored"
)
```

This foreground check uses the backed-up binary and restored data. Reopen the
browser/tunnel and sign in with the existing password. Stop it with Ctrl+C after
verification. Old cookies must not regain access. An invalid record or unsafe
permissions must stop startup; do not repair it by deleting the record or running
setup. A lost password cannot be recovered from this backup.

To adopt the restore, point **both** `ExecStartPre` and `ExecStart --data-dir` in
the user unit at the verified restored directory, and `ExecStart` at the chosen
compatible binary. Verify the edited unit, run `systemctl --user daemon-reload`,
then start and check it. Keep the old directory until recovery is confirmed.
To abandon the drill, leave the unit unchanged and start it against the original
data. Restoring on a different host/OS account and cross-version migrations have
not been tested; do not assume copied ownership or formats will be accepted.

## Validation and remaining gaps

The Ubuntu 24.04 CI job runs the following unprivileged checks in temporary
directories. Check the actual CI result for the revision you intend to install:

```sh
cargo build --release --locked --offline
cargo test -p devoption-cli --test server --release --locked --offline installation
python3 scripts/check_systemd_unit.py target/release/devoption
```

The release fixture copies the executable outside the checkout, signs in, stops,
backs up and restores the owner record, signs in after restart, rejects the old
session, checks record bytes/permissions, and rejects a damaged restore without
listening. Existing workspace tests exercise CLI setup through temporary PTYs.
The systemd check only verifies unit syntax/executable paths and the missing or
empty owner preflight in a disposable home; it installs or starts no service.
See [systemd's verifier](https://github.com/systemd/systemd/blob/v255/man/systemd-analyze.xml).

CI fixtures do not by themselves validate a real user manager or SSH connection.
A separate [Ubuntu 24.04 x86_64 guest rehearsal](linux-rehearsal.md) checked source
installation, user-service start/restart, SSH browser access, backup/restore,
complete logout and recovery after reboot and login. Its image, source revision,
observed behavior, disk use and cleanup are recorded there. A bounded
[lingering follow-up](linux-unattended-rehearsal.md) separately checked startup
before login in that same guest and restored the original policy. Other Linux
distributions and architectures, unattended operation on other hosts, public
hosting, full project/build/deploy flows, and disaster recovery for future state
remain outside this guide's validated scope.
