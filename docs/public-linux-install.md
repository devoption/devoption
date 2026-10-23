# Single-node platform installation with HTTPS

For installation without compiling DevOption, see the
[verified bundle installer](bundle-installation.md). It currently requires an
explicit validation candidate and preserves the host/publication approval gates.
The source-based path below remains available.

This is the first public-platform slice of
[#230](https://github.com/devoption/devoption/issues/230). The server now supports
one configured HTTPS origin behind a local proxy. Projects, login and restart are
covered by disposable integration tests and a [clean local VM rehearsal](clean-local-rehearsal.md),
including reboot. A [fresh cloud acceptance run](public-cloud-acceptance.md) also
verified source installation, public DNS and ACME issuance/accelerated renewal.
[App builds](web-app-builds.md), [deployment](web-app-deployments.md) and
a [second app hostname](app-hostnames.md) are available as separate opt-in steps.
The [private installation](private-linux-install.md) remains available unchanged.

## Supported shape and prerequisites

Use a fresh Ubuntu 24.04 x86_64 VM, one dedicated `devoption` OS account, Caddy
**2.11.7**, and one DNS hostname such as `platform.example.com`. Replace that example
with your own lowercase ASCII DNS name in both configuration files. Public mode
supports HTTPS on port 443 only; no path prefix, IP address, wildcard or alternate
external port. Use an ASCII/punycode hostname for internationalized domains.

The operator supplies the VM, administrator/SSH access, DNS control and approvals
for installation, OS accounts and owner setup. These instructions are not authority
for an agent to purchase a VM, create real credentials or change public DNS.
Install Git, a C compiler/linker, curl, tar, Python 3.11+, and rustup with Rust
1.99.0. Record Ubuntu package versions and the VM image identity. Caddy installation
below uses an exact upstream archive/checksum; Rust and Cargo dependencies use the
repository pins. No product release or package registry is required.

Allow TCP 80 and 443 to Caddy, and restrict SSH to your administrative access.
Do not expose TCP 3000. Point the hostname's A record to the VM's public IPv4.
Add AAAA only if IPv6 reaches this same server and its firewall permits HTTPS/HTTP.
Remove stale AAAA records. Check DNS from outside the VM before starting Caddy.
Caddy needs outbound DNS and HTTPS for certificate issuance/renewal. No DNS-provider
API credentials are required by this HTTP/TLS challenge setup.

The proxy allows 95 seconds for response headers so bounded build/deployment
approvals can finish, including failed health checks before an app hostname is
configured. Keep this allowance when adapting the example.

The proxy preserves Host and Origin and strips all forwarding metadata. DevOption
never uses `Forwarded`, `X-Forwarded-*` or `X-Real-IP` to infer scheme, host or client
identity; public-mode requests containing them at the backend are rejected. Only
trusted local processes may reach the backend. This TCP loopback arrangement does
not authenticate the proxy against other local users: use a dedicated VM without
untrusted shell users or workloads. Future app hosting must preserve this boundary.

## Optional protected administrative access

The standard proxy above provides HTTPS plus DevOption owner authentication.
For a device-certificate gate before any administrative HTTP content, use the
review-only hostname generator with Caddy **2.11.7** client authentication. This
is an operator-managed mTLS access policy, not browser SSO, team enrollment or a
claim of comprehensive zero-trust certification.

The gate requires both a chain verified against the configured private client CA
and an exact allowed client leaf certificate. Owner sign-in, session expiry and
Origin/CSRF checks still apply after TLS admission. A certificate alone never
creates a DevOption session or owner. HTTP identity headers are not credentials;
the proxy strips client/SSL identity and forwarding header families upstream.
Public app hostnames have their own TLS policy and do not request client certs.

### Prepare certificate inputs separately

Use your approved certificate issuance process to obtain a client CA certificate
and one distinct client certificate/key per authorized device. Certificates need
client-auth usage and a short, maintained validity period. Import the device's
certificate/private key through the browser or OS certificate manager, using a
protected, encrypted export format when transport is necessary. Test on the actual
browser/device before relying on this route. No certificate issuance or enrollment
is performed by the generator, installer or server. Do not put a CA private key or
device private key on the proxy; Caddy only needs public certificates.

Install the client CA PEM and a PEM bundle containing only explicitly permitted
client leaf certificates at these example paths. Keep both files and every parent
root-owned and non-writable by the platform, apps or other users; Caddy must be
able to read them. For example, after reviewing the public certificate inputs:

```sh
sudo install -d -m 755 -o root -g root /etc/caddy/access
sudo install -m 644 -o root -g root client-ca.pem /etc/caddy/access/client-ca.pem
sudo install -m 644 -o root -g root allowed-clients.pem /etc/caddy/access/allowed-clients.pem
```

Do not use the public server certificate CA bundle as the client CA. The client CA
and public HTTPS server certificate serve separate purposes. Public server TLS
continues using normal ACME issuance/renewal; never disable server verification.

Keep this small policy file in private operator configuration:

```toml
client_ca_file = "/etc/caddy/access/client-ca.pem"
allowed_clients_file = "/etc/caddy/access/allowed-clients.pem"
```

Only those two fields and canonical absolute ASCII file-path syntax are accepted.
The generator does not inspect or authenticate the files at those target paths;
the real proxy's validation/startup checks the certificate inputs. A missing or
invalid input cannot fall back to ungated access. An empty allowlist accepts no
clients (or refuses activation), rather than trusting every certificate from the CA.

### Generate and activate a protected route

For a platform without an app binding, save that policy as `platform-access.toml`:

```sh
python3 scripts/configure_app_hostname.py \
  --platform-only \
  --platform-hostname platform.example.com \
  --platform-port 3000 \
  --platform-access-policy ./platform-access.toml \
  --output ./protected-proxy-review
```

This creates a new private review directory and `Caddyfile`; it installs nothing,
issues no credentials and changes no DNS. Existing output is never overwritten.
`--platform-only` requires protection. When adding an app, use the same policy with
[the two-host generator](app-hostnames.md); keep the policy under operator control
and pass it on every regeneration. The alternative `--public-platform` is an
explicit choice to generate an administrative route without a certificate gate.
Do not select it to work around a certificate or recovery problem.

Keep backend ports private, the dedicated-node trust boundary above, strict
SNI/Host checking, and the existing service/storage/owner settings. Review and back
up the current proxy configuration and public certificate-policy inputs. Validate
as the actual Caddy service user before replacement:

```sh
test ! -e /etc/caddy/Caddyfile.review
test ! -L /etc/caddy/Caddyfile.review
sudo install -m 644 -o root -g root ./protected-proxy-review/Caddyfile /etc/caddy/Caddyfile.review
sudo -u caddy env XDG_DATA_HOME=/var/lib/caddy/data XDG_CONFIG_HOME=/var/lib/caddy/config \
  /usr/local/bin/caddy adapt --config /etc/caddy/Caddyfile.review --adapter caddyfile --validate >/dev/null
```

The staging file is root-controlled and Caddy-readable; the original review
directory remains private. Validation must read the installed certificate inputs
successfully. Do not activate an invalid configuration. After retaining the
reviewed prior configuration and certificate-policy inputs, install this validated
staging file at `/etc/caddy/Caddyfile` with root ownership/mode 0644 and restart the
existing Caddy service; its admin API remains disabled. Perform allowed and denied external
checks before treating the installation as ready. A full proxy restart can briefly
interrupt the app route too; it does not stop or redeploy the app worker.

### Revoke, rotate and recover

Remove a device's exact leaf certificate from the allowlist, validate the intended
configuration and **restart the proxy** to end old connections and TLS sessions.
Merely editing a PEM file does not update a running verifier. Prove the removed
device is denied even with its existing platform cookie, another permitted device
can still sign in, and the public app still works. No CRL/OCSP lookup or automatic
device enrollment is added. For rotation, temporarily allow the reviewed old and
new leafs, test the new device, then remove the old leaf and restart again.

Maintain independent administrator SSH/console access. Recover by restoring a
reviewed CA/allowlist or authorizing a replacement device through the established
issuance process, rather than exposing the backend or removing client auth. Avoid
restoring revoked identities unintentionally. Back up the access policy, CA public
certificate, allowed leaf bundle and reviewed Caddyfile separately: the bounded
app-state snapshot does not collect these proxy files or device/issuer keys.

This optional source/configuration profile does not change the signed installer's
default public proxy template. Its current base-upgrade/reinstall contract expects
managed configuration bytes; it may refuse a customized protected installation.
Do not replace the gate with the default template to pass that check. Protected
profile upgrade acceptance remains separate.

### Validation scope

The ignored `public_https_proxy` integration test exercises ordinary and protected
routes through the pinned Caddy binary and an actual configured DevOption server.
Disposable certificates cover permitted-device owner login, absent/untrusted/
unlisted/expired/wrong-usage denial, forged-header and cross-host attempts, missing
and empty trust inputs, revocation/restart with an otherwise valid session cookie,
and independent certificate-free app access. CI runs this explicitly on Linux.
The test uses local server trust only and removes its temporary keys/trust; it does
not establish public DNS, ACME or real browser-device enrollment for a new host.

Upstream references: [Caddy client authentication and leaf verifier](https://caddyserver.com/docs/caddyfile/directives/tls#client_auth)
and the [pinned connection-policy implementation](https://github.com/caddyserver/caddy/blob/v2.11.7/modules/caddytls/connpolicy.go).

## Build the reviewed source

As the administrator's unprivileged build user, select the reviewed commit:

```sh
git clone https://github.com/devoption/devoption.git
cd devoption
git checkout --detach REVIEWED_COMMIT
rustup toolchain install 1.99.0 --profile minimal
cargo fetch --locked
cargo build --release --locked --offline
```

Replace `REVIEWED_COMMIT` with the full commit reviewed for installation and verify
its CI results. Fetching downloads dependencies; do it explicitly before the offline
build. No managed-project workload is executed by installation.

## Install the platform account and data

The following block is for a **fresh** installation only. It stops if either
account or destination exists. Inspect an existing installation instead of deleting
or overwriting it to retry. Run from the reviewed repository checkout:

```sh
(
  set -eu
  if getent passwd devoption || getent group devoption; then exit 1; fi
  for path in /opt/devoption /var/lib/devoption; do
    test ! -e "$path"
    test ! -L "$path"
  done
  sudo useradd --system --user-group --create-home --home-dir /var/lib/devoption --shell /usr/sbin/nologin devoption
  sudo chmod 700 /var/lib/devoption
  sudo install -d -m 700 -o devoption -g devoption /var/lib/devoption/data /var/lib/devoption/repositories
  sudo install -d -m 755 -o root -g root /opt/devoption
  sudo install -m 755 -o root -g root target/release/devoption /opt/devoption/devoption
  git rev-parse HEAD | sudo tee /opt/devoption/source-commit.txt
)
sudo -u devoption /opt/devoption/devoption setup --data-dir /var/lib/devoption/data
```

Setup requires an interactive terminal (use SSH with `-t`); enter and confirm a
unique passphrase of at least 15 characters and wait for `Owner created.`. Never
put the password in shell arguments, environment or a script. There is no public
owner enrollment or reset endpoint. If setup fails or is interrupted, inspect
existing state; never delete `owner.bin` to make setup succeed.

Copy [devoption-public.service](../examples/systemd/devoption-public.service) to a
local working file and replace `https://platform.example.com` with your canonical
origin. Keep `--data-dir`, the existing-owner preflight, private repository root,
`User`/`Group` and loopback port. Install only after reviewing the resulting file:

```sh
(
  set -eu
  test ! -e /etc/systemd/system/devoption-public.service
  test ! -L /etc/systemd/system/devoption-public.service
  sudo install -m 644 devoption-public.service /etc/systemd/system/devoption-public.service
  sudo systemd-analyze --man=no --recursive-errors=no verify /etc/systemd/system/devoption-public.service
  sudo systemctl daemon-reload
  sudo systemctl enable --now devoption-public.service
  sudo systemctl status devoption-public.service --no-pager
  curl -I -H 'Host: platform.example.com' http://127.0.0.1:3000/projects
)
```

Replace the Host value too. Expect a 303 redirect to `/sign-in`. The HTTP backend
is only for this local diagnostic/proxy hop; open the HTTPS hostname in a browser.
An existing owner is mandatory at runtime even if the preflight is bypassed.
Missing/corrupt owner or catalog data fails startup rather than serving the preview.
This is a system unit with an unprivileged account and boot enablement; no user
login or lingering is needed. The unit grants no build or app-execution capability.

To supply source later, stage a trusted checkout as `devoption` inside the
configured private repository root, then attach it in the UI. Do not copy SSH
keys into that account or change ownership of a personal checkout. The initial
unit grants inspection/attachment only, not UI-created repository directories.

## Install the pinned proxy and configure TLS

Caddy is separate from the platform account. On a fresh VM with no existing Caddy
installation, download and verify the official Linux archive in a temporary folder:

```sh
(
  set -eu
  caddy_stage=$(mktemp -d)
  cd "$caddy_stage"
  curl --fail --location --proto '=https' --tlsv1.2 -o caddy.tar.gz https://github.com/caddyserver/caddy/releases/download/v2.11.7/caddy_2.11.7_linux_amd64.tar.gz
  printf '%s  caddy.tar.gz\n' a7a433a1b133efc3c8d10eb0b99d52a24b5ef5c322dc77f5282182b1c0402139ab83f3a99f0c52409df77d20123fb0b523edad8a66d8f5e49136197bf61ef0e7 | sha512sum --check -
  tar -xzf caddy.tar.gz caddy
  if getent passwd caddy || getent group caddy; then exit 1; fi
  for path in /usr/local/bin/caddy /var/lib/caddy /etc/caddy; do
    test ! -e "$path"
    test ! -L "$path"
  done
  sudo useradd --system --user-group --create-home --home-dir /var/lib/caddy --shell /usr/sbin/nologin caddy
  sudo chmod 700 /var/lib/caddy
  sudo install -d -m 700 -o caddy -g caddy /var/lib/caddy/data /var/lib/caddy/config
  sudo install -d -m 755 -o root -g root /etc/caddy
  sudo install -m 755 -o root -g root caddy /usr/local/bin/caddy
  /usr/local/bin/caddy version
  rm caddy caddy.tar.gz
  cd /
  rmdir "$caddy_stage"
)
```

Use the [example Caddyfile](../examples/caddy/Caddyfile), replacing its hostname,
and [Caddy unit](../examples/systemd/caddy-devoption.service). Do not add a catch-all
proxy or replace Host/Origin. The example removes Caddy's generated forwarding
headers as well as those supplied by callers. TLS SNI and HTTP Host must match. The admin API is disabled; restart
the proxy after configuration changes rather than using `caddy reload`.

```sh
(
  set -eu
  test ! -e /etc/caddy/Caddyfile
  test ! -L /etc/caddy/Caddyfile
  sudo install -m 644 Caddyfile /etc/caddy/Caddyfile
  test ! -e /etc/systemd/system/caddy-devoption.service
  test ! -L /etc/systemd/system/caddy-devoption.service
  sudo install -m 644 examples/systemd/caddy-devoption.service /etc/systemd/system/caddy-devoption.service
  sudo -u caddy env XDG_DATA_HOME=/var/lib/caddy/data XDG_CONFIG_HOME=/var/lib/caddy/config /usr/local/bin/caddy adapt --config /etc/caddy/Caddyfile --adapter caddyfile --validate >/dev/null
  sudo systemd-analyze --man=no --recursive-errors=no verify /etc/systemd/system/caddy-devoption.service
  sudo systemctl daemon-reload
  sudo systemctl enable --now caddy-devoption.service
  sudo systemctl status caddy-devoption.service --no-pager
  curl -I https://platform.example.com/projects
)
```

Expect a trusted certificate and 303 to `/sign-in`. Caddy automatically obtains
and renews certificates for the configured public hostname and redirects HTTP to
HTTPS. Its state and private keys persist in `/var/lib/caddy`; do not erase that
state on restart or reinstall. Do not use `tls internal` or bypass certificate
verification on the real server. The test fixture's private CA is not a public
installation option. Verify issuance and renewal for your own installation; the
[recorded cloud run](public-cloud-acceptance.md#external-routing-and-certificate-lifecycle)
does not establish another node's certificate lifecycle.

Open `https://platform.example.com`, sign in, create a named project, reload the
page and sign out. A private browser window must not access saved project content.
Session cookies are host-only `__Host-` cookies with Secure, HttpOnly and
SameSite=Strict; mutations still require the matching HTTPS Origin and CSRF token.
Appearance/settings continue to use the existing UI; no public setup page is added.

## Failure diagnosis and restart verification

- **TLS/DNS failure:** check A/AAAA records, inbound 80/443, outbound DNS/HTTPS,
  CAA restrictions and Caddy logs. Allow propagation; repeated reinstalls can hit
  certificate issuance limits. Do not weaken TLS or switch the backend to public HTTP.
- **502:** check `devoption-public.service`, its private port and owner/data errors.
  `systemctl start` is not a readiness check. Inspect bounded recent logs with
  `journalctl -u devoption-public.service -n 50 --no-pager`.
- **400:** compare canonical origin and the proxy's Host; check that every
  forwarding header is stripped. Requests sent directly with loopback Host are
  deliberately rejected in public mode. Do not turn on `trusted_proxies` or rewrite
  Host to `127.0.0.1`.
- **403:** confirm browser HTTPS origin and session/CSRF state; do not configure CORS
  to bypass it. **429/503 at sign-in:** the existing shared authentication admission
  limits apply across all callers; wait for the response's retry hint. Public use
  does not add client-IP-based rate limiting.
- **Startup failure:** ownership, permissions, source paths and the existing owner
  must be valid. Never run the platform as root or delete state as a recovery shortcut.

Restart DevOption and verify that old sessions are rejected, a new login works,
and the saved project remains. Then reboot the VM and verify both services and
HTTPS access before relying on unattended operation. Record the exact source/OS/
proxy versions and outcomes. A reboot test is still required on the actual host;
process-restart integration tests do not establish systemd/boot behavior.

For a source upgrade, build and review the new binary first, stop the platform,
retain the prior binary and commit receipt, install the new binary and start it.
Keep data unchanged and verify login/project state; no migration compatibility is
promised. Configuration changes require unit verification and `daemon-reload`.

Back up the owner, catalog and profile as a consistent offline snapshot using
[the private guide's state rules](private-linux-install.md#restart-backup-and-restore),
substituting this system unit and `/var/lib/devoption/data` paths. Preserve UID/GID,
private modes, source receipt and compatible binary. Back up repositories and
Caddy state separately; Caddy keys and owner hashes are private material. The
initial unit has no build-history store enabled. Do not copy ephemeral sessions or
writer locks. Restore into fresh private directories and validate before selecting
them; never overwrite the only working state or assume cross-version compatibility.

To uninstall, disable/stop `caddy-devoption.service` and `devoption-public.service`,
remove only the installed unit/config/binary files after inspection, and reload
systemd. Preserve `/var/lib/devoption`, `/var/lib/caddy`, repository contents and
backups. Do not delete accounts or data automatically. Remove DNS separately when
authorized so it does not later point at an unrelated recycled IP.

## Evidence and repeatable checks

The workspace suite exercises strict configuration, Host/Origin/forwarding-header
rejection, existing-owner enforcement, Secure cookies, login/logout, project
creation, session invalidation and state preservation after process restart.
The Linux static unit check validates the new examples without starting services.

With an explicitly supplied verified Caddy 2.11.7 binary, run:

```sh
DEVOPTION_TEST_CADDY=/absolute/path/to/caddy cargo test -p devoption-cli --test server --locked --offline public_https_proxy -- --ignored --nocapture
```

This uses synthetic owner records and temporary data in the test process, a loopback-only Caddy TLS
listener and temporary CA, verifies TLS against that CA, tests login/project
creation/logout and proxy-header stripping, and removes its files. It installs
no trust root, calls no public ACME service and changes no DNS or system service.
It passed locally on macOS with the upstream arm64 archive verified against its
published SHA-512. The [Ubuntu system-service rehearsal](public-linux-rehearsal.md)
also verified local-CA HTTPS, service restart and startup before login after reboot.
The later [public cloud acceptance](public-cloud-acceptance.md) records actual
source installation, public DNS/ACME and lifecycle checks for #230/#234.

Upstream references: [Caddy reverse-proxy headers](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#headers),
[automatic HTTPS](https://caddyserver.com/docs/automatic-https),
[release 2.11.7 and checksums](https://github.com/caddyserver/caddy/releases/tag/v2.11.7),
[systemd services](https://www.freedesktop.org/software/systemd/man/255/systemd.service.html),
and [systemd execution restrictions](https://www.freedesktop.org/software/systemd/man/255/systemd.exec.html).
