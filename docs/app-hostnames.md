# Give the deployed app its own HTTPS hostname

This extends the [platform HTTPS installation](public-linux-install.md) and
[private app deployment](web-app-deployments.md). One Caddy 2.11.7 installation
serves the platform and one stateless Wasm app on the same IP and ports 80/443.
The operator manages the hostname binding; the project Releases page displays
its app URL and checks the local HTTPS route and certificate on refresh.

## Generate and review the binding

Start with the installed deployment configuration and the project's 32-character
ID from its URL. Choose two distinct lowercase ASCII DNS names. The app name must
belong to this operator; this configuration check does not prove public DNS ownership.
Only one app hostname/project binding is supported. IPs, wildcards, paths, ports,
duplicate platform/app names and arbitrary proxy directives are rejected.

```sh
python3 scripts/configure_app_hostname.py \
  --deployment-config /etc/devoption/deployment.toml \
  --platform-hostname platform.example.com \
  --app-hostname app.example.com \
  --project YOUR_32_CHARACTER_PROJECT_ID \
  --platform-port 3000 \
  --output ./hostname-review
```

Use the existing platform's actual loopback port. The command creates a new
review directory containing `deployment.toml` and `Caddyfile`; it installs nothing,
starts no app and changes no DNS. Existing output directories are never overwritten.
It preserves the deployment storage fields and stages a replacement binding when
one already exists. Inspect both generated files before installation.

The deployment configuration gains this table:

```toml
[ingress]
platform_hostname = "platform.example.com"
app_hostname = "app.example.com"
project = "YOUR_32_CHARACTER_PROJECT_ID"
platform_port = 3000
```

Replace the placeholder with a real saved project ID. At startup the platform
requires the binding to match its `--public-origin`, actual port and saved project.
The broker refuses to deploy another project's release on the bound app port.
The binding is part of the reviewed runtime fingerprint: installing or changing it
requires a fresh deployment review. Existing release bytes remain unchanged.

## DNS, installation and certificates

Point **both** A records to the same server. Add AAAA records only when that
server's IPv6 ingress works; remove stale AAAA records. Keep ports 3000 and the app
worker's port private. Open only the already documented SSH/80/443 ingress.
Do not add a wildcard or catch-all proxy to the administrative interface.

Retain the platform service's owner/storage/build/deployment options and set its
canonical `--public-origin https://platform.example.com`. Keep the existing Caddy
service and persistent certificate storage from the installation guide. Back up
the current configuration files before installing this reviewed pair:

```sh
sudo install -m 644 hostname-review/Caddyfile /etc/caddy/Caddyfile
sudo -u caddy env XDG_DATA_HOME=/var/lib/caddy/data XDG_CONFIG_HOME=/var/lib/caddy/config \
  /usr/local/bin/caddy adapt --config /etc/caddy/Caddyfile --adapter caddyfile --validate >/dev/null
sudo systemctl stop devoption-public.service
sudo install -m 644 hostname-review/deployment.toml /etc/devoption/deployment.toml
sudo systemctl daemon-reload
sudo systemctl restart caddy-devoption.service
sudo systemctl start devoption-public.service
```

Use the installation's actual unit names. On failed validation, restore the prior
Caddyfile before restarting. Caddy's admin API stays disabled; configuration changes
use service restart. Sign in again, open the bound project's Releases tab, and
review/deploy the saved release. A changed fingerprint never silently resumes it.

Caddy's named public sites enable automatic certificate issuance and renewal.
Preserve its writable certificate state across service/host restarts. Verify A/AAAA,
CAA, inbound 80/443 and outbound DNS/HTTPS if issuance fails. Inspect bounded Caddy
logs before retrying; repeated reinstalls can hit CA rate limits. Never use
`tls internal`, a rehearsal CA, or disabled certificate verification in production.
The [public cloud acceptance](public-cloud-acceptance.md) records real public DNS,
issuance and accelerated renewal. Local CA evidence alone cannot establish those
results; verify each installation separately.

## Use and diagnose the route

On Releases, **Open app** opens the configured HTTPS hostname. **Refresh health and
logs** probes only `127.0.0.1:443`, using that hostname for TLS SNI and Host. A verified
route requires a trusted, hostname-valid certificate, HTTP 200, the exact approved
health body and the selected worker invocation. It follows no redirects and reads
a bounded body. This does **not** query public DNS or prove external reachability.

- **Certificate verification failed:** check hostname, expiry, trust chain and
  Caddy issuance/renewal logs. Do not bypass verification.
- **Ingress unavailable:** check Caddy's service, listener and TLS configuration.
- **Proxy cannot reach app:** check the app worker and generated private upstream.
- **Route does not match:** compare the configured hostname, port and expected
  health response. A different process on that port is not accepted as healthy.
- **App stopped/not deployed:** review and deploy a saved release. While stopped,
  the app hostname can return 502; the platform remains usable.

The hostname stays stable when deploying B or returning to saved A. Caddy routes
to the same fixed loopback port; release changes do not rewrite proxy configuration.
After platform/host restart, the existing deployment recovery policy applies.
Check both URLs after reboot, and verify an unknown hostname cannot reach either
application. A stopped app must remain stopped.

Platform cookies remain host-only Secure `__Host-` cookies. The app proxy removes
Cookie, Authorization, Proxy-Authorization and control/forwarding headers; the Wasm
host strips control credentials again. `/auth`, `/auth/*`, `/sign-in`, `/setup` and
`/setup/*` are reserved and return 404 at the app proxy. Other app paths never route
to the platform. This first stateless app profile does not support app authentication.

Use the [external HTTPS verification command](first-web-app.md#external-https-verification)
from an external client to save repeatable per-address routing checks after
installation, updates and reboot. Its results supplement the browser and
certificate-lifecycle evidence; they do not replace full public acceptance.

## Local validation

The existing ignored `public_https_proxy` integration test now uses the generated
two-site proxy configuration. It covers authenticated platform access, independent
app routing, stripped cookies, blocked control paths, mismatched SNI/Host,
unknown TLS names and changing app responses without a hostname change.

For a disposable rehearsal only, add `tls internal`, `bind 127.0.0.1`,
`auto_https disable_redirects` and `skip_install_trust` to the staged Caddyfile.
An optional `ca_file` in `[ingress]` selects one explicit root-owned, protected,
bounded PEM trust file for the local status probe. Its bytes are part of the
runtime fingerprint. It never installs system trust and the UI labels this state
as rehearsal verification. Omit it in production to use the system trust store.

The contribution checks and `scripts/test_app_hostname.py` cover strict binding
validation, generated file preservation, wrong-project deployment rejection and
certificate/upstream/mismatched-release status. See the screenshot gallery for
real local-VM UI captures; screenshots alone do not prove public reachability.

### Observed Ubuntu rehearsal

The disposable Ubuntu 24.04 x86_64 VM served `platform.example.test` and
`app.example.test` on its one loopback HTTPS listener with Caddy 2.11.7. Existing
owner credentials and real saved Wasm builds were reused. Browser sign-in, Open
app, update A → B and explicit return to saved A succeeded without changing the
hostname. The browser sent no platform cookie to the app. Separate full-chain TLS
checks passed both URLs, unknown-host rejection, SNI/Host mismatch and blocked
control paths. Deliberately untrusted certificate and unavailable-upstream cases
produced their documented UI messages, then recovered after restoring configuration.
Stopping the app returned 502 on its hostname while platform HTTPS remained usable.
Both hostnames and saved A recovered after service restart and a full VM reboot;
login worked again and the route verified the new worker invocation. Startup briefly
returned 502 before the worker was ready, so service-active state alone was not
counted as readiness. No new application build was started by recovery.

References: [Caddy automatic HTTPS](https://caddyserver.com/docs/automatic-https)
and [reverse proxy header controls](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#headers).
