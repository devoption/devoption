# Ubuntu HTTPS service rehearsal — 2026-10-16

The follow-up under [#230](https://github.com/devoption/devoption/issues/230)
reused the existing disposable [Ubuntu guest](linux-rehearsal.md) to exercise the
[public installation examples](public-linux-install.md). It verified local HTTPS
and real system services. Public DNS, ACME issuance/renewal and cloud networking
remain unverified; #230 remains open.

## Environment and adaptations

- Ubuntu 24.04.5 x86_64, systemd 255 (255.4-1ubuntu8.17), QEMU TCG emulation,
  two virtual CPUs and 2 GiB RAM.
- Rust 1.99.0; `cargo fetch --locked`, followed by
  `cargo build --workspace --release --locked --offline -j2`. The build reused
  the guest's Cargo cache and target directory; this was not a clean VM install.
- Source commit
  [`97297689b652b8a02ab8bdad44d0eb14fad697f5`](https://github.com/devoption/devoption/commit/97297689b652b8a02ab8bdad44d0eb14fad697f5),
  whose tracked tree matches merged commit
  [`4b55dad7a9abf566a7e0e711eed67b91d5b6191d`](https://github.com/devoption/devoption/commit/4b55dad7a9abf566a7e0e711eed67b91d5b6191d).
  Documentation screenshots were omitted from the guest staging directory.
- Caddy 2.11.7 Linux amd64 archive, verified against the release's SHA-512 checksum.
- The existing disposable owner record and password were reused. Fresh CLI owner
  enrollment was not repeated. No owner record, password, key or session cookie is
  included in this report.

The previously absent installation paths and dedicated `devoption` and `caddy`
system accounts were created for this test. The existing private user service
was stopped before installation; its unit, owner data and backed-up binary were
preserved. The new system units were verified with `systemd-analyze verify`,
installed and enabled under `multi-user.target`.

The DevOption unit used `https://platform.example.test` instead of the example's
public hostname. The Caddy example used the same hostname, with `bind 127.0.0.1`,
`tls internal`, `auto_https disable_redirects` and `skip_install_trust` added.
This kept HTTPS inside the guest and avoided public ACME and system trust changes.
The proxy header-stripping, strict SNI/Host check and service protections were
retained. The probe explicitly trusted the temporary CA and sent the configured
TLS SNI, Host and Origin; it did not disable certificate verification.

## Observed results

| Check | Result |
| --- | --- |
| Service installation | Both units active and enabled, running as their respective dedicated accounts |
| Listener boundary | DevOption on `127.0.0.1:3000`; Caddy HTTPS on `127.0.0.1:443` |
| Anonymous access | Protected projects page redirected with `303` |
| Login and logout | Existing fixture password accepted; Secure, HttpOnly, SameSite=Strict cookie without Domain; logout invalidated access |
| Project creation | Authenticated project creation succeeded and appeared on the projects page |
| Origin and proxy boundary | Cross-origin mutation rejected with `403`; mismatched SNI/Host rejected with `421`; supplied forwarding metadata stripped before reaching DevOption |
| Platform service restart | Project remained available after login; previous session rejected with `303` |

A temporary oneshot system unit ordered after both services recorded verified
HTTPS readiness at **20.45 seconds after boot with zero login sessions**. Its boot
ID matched the running guest. The login-manager journal placed the first login at
**78.243392 seconds**, after the probe. The DevOption service had started at
17.244951 seconds and Caddy at 16.274813 seconds. Both remained active and enabled.
After reboot, the previous session was rejected, the existing password worked,
and the created project remained available. The installed owner record was
byte-identical to the preserved original. The fixture account retained `Linger=no`;
the system services did not require user lingering.

An earlier reboot check connected over SSH before the probe finished and was not
used as evidence of startup before login. The disconnected repeat above supplied
that evidence.

These were protocol-level HTTPS checks, not a new browser interaction recording.
The local build, workspace tests, format, Clippy, Python and Node checks also
passed. No application build, Wasm runtime or web-app deployment was exercised.

## Cleanup and resource budget

The temporary system units, installed binaries/configuration, service accounts,
fixture installation data and local CA were removed after testing. The original
private user unit remained enabled with `Linger=no`; its owner and backup binary
were retained. The VM was powered off, and its monitor and associated processes
were confirmed stopped. Ignored source/build staging remains for later reuse.

The existing 4 GiB allocated-artifact limit and 20 GiB host-free-space floor were
retained. The monitor recorded a peak of 3,838,398,464 allocated bytes; no budget
stop occurred. No virtual disk expansion was needed.

## Remaining cloud acceptance

On the actual host, follow the installation guide with a real platform hostname
and public DNS. Verify ingress, automatic certificate issuance, browser login,
project persistence and reboot recovery there. This local CA rehearsal does not
establish public DNS propagation, ACME validation/renewal, cloud firewall behavior,
or successful installation on a completely fresh image.
