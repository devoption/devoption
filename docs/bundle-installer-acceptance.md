# Bundle installer acceptance

The 2026-10-19 rehearsal for [#262](https://github.com/devoption/devoption/issues/262)
used the merged installer from commit
`0b9259435f484e8d80bb40cd128ab2d7bdadde41`, not a source-built replacement for
the retained executable. The results below supplement the automated fixtures in
[the installer guide](bundle-installation.md#validation-scope).

## Inputs and environment

The disposable guest was Ubuntu 24.04.5 amd64, kernel `6.8.0-146-generic`,
systemd `255.4-1ubuntu8.17`, Python 3.12.3 and Git 2.43.0. It used QEMU software
emulation, cgroup v2 and a new copy-on-write disk over the Ubuntu minimal cloud
image with SHA-256
`2f119f50bf29fe40e73b69ae2a736e8210d8146796f036e0324315b6b3a777df`.
The guest started without DevOption accounts, units or installation directories.
Only documented OS prerequisites and the independently pinned GitHub CLI were
prepared before testing. No Rust toolchain or compiler was installed.

The trusted installer scripts/reservation were transferred from that exact
commit into an administrator-owned directory. The guest received no GitHub
token, host filesystem share or forwarded SSH agent. The downloaded packet was
verified inside the guest with the real GitHub CLI 2.102.0 and production
provenance policy, without mocking cryptographic success.

| Input | Identity |
| --- | --- |
| Reservation | `release-candidates/0.1.0-preview.1.json`, SHA-256 `f72dac7b4caca33234632a5588ca88c3c368f78c3240db56526248c741d7de8d` |
| Retained packet | Asset `656905392`, SHA-256 `c3c8ecd8e6d986397243b9be70a6c658eec3bcb5e3992fff20a435e4c3d688c3` |
| Signed bundle source | `a11c0619d98c14f43b41e3947e7f4f00a71dc2f0` |
| Inner archive | SHA-256 `0ee462ffb9048fbc08108693a5f17167516a5d4c676a71659f1601470aefd0a3` |
| Verifier archive | GitHub CLI 2.102.0 Linux amd64, SHA-256 `bb766f710eef8ede859c18578c72c327597cd4c8a85b06001b1f3843c6019386` |
| Proxy | Caddy 2.11.7 official archive; production SHA-512 pin checked against both trusted code and authenticated manifest |

Every installer mode selected the same reservation, packet, verifier and Caddy
archive, `--hostname platform.installer.localhost` and
`--allow-validation-candidate`. The installed binary was
`/opt/devoption/releases/0.1.0-preview.1/bin/devoption`.

## Observed results

| Case | Result |
| --- | --- |
| Corrupt download | Flipping one byte in a copy of the packet caused verification failure before accounts, receipt or installation directories existed. The unchanged signed packet passed. |
| Occupied port | A real unrelated listener on TCP 3000 caused preflight rejection; the listener remained open and installation resources remained absent. |
| Insufficient storage | A temporary 16 MiB tmpfs over the guest's empty `/opt` caused preflight rejection before installation. The test unmounted its own tmpfs afterward. |
| Interrupted installation | A test wrapper called production `Installer.write`, then exited with code 75 after six completed atomic writes, including the receipt. The normal, unmodified `install` command resumed and reported no missing files. All six committed files retained their inode, digest, ownership, mode and link count. This was controlled fault injection, not simulated disk corruption or a power failure. |
| Real host installation | Production account creation, file placement, systemd unit validation, Caddy validation and daemon reload succeeded as root. Both service accounts were non-login identities. Persistent files belonged to `devoption`, with private data directories and mode-600 owner/project/profile records. |
| Interactive owner setup | `enroll` ran over an SSH PTY as `devoption`, requested password and confirmation, and produced a private owner record. An existing disposable fixture passphrase was entered through the terminal; it was absent from command arguments, environment and captured output. |
| Start and isolation | `start` enabled and started both real services. Each ran under its dedicated identity with `ProtectSystem=strict` and `NoNewPrivileges=yes`. The backend listened only on `127.0.0.1:3000`; Caddy owned 80/443. Anonymous `/projects` returned 303 to `/sign-in` through HTTPS. |
| HTTPS application state | Certificate and hostname verification succeeded with an explicitly selected local test root. Sign-in established a Secure/HttpOnly session. Authenticated requests created a project and profile, read them back and signed out; subsequent session/project requests were denied or redirected. These were HTTPS protocol checks, not browser UI automation. |
| Identical retry | A further `install` and noninteractive `enroll` preserved the existing owner. All 35 installed/receipt files retained inode, digest, ownership, mode and link count. Owner, project and profile hashes were unchanged. |
| Service restart | Both services returned active; saved project/profile and owner login worked through verified HTTPS. The previous session was invalidated. |
| Guest reboot | A changed kernel boot ID established a real reboot. Both services returned active and enabled without manual startup; verified HTTPS, owner login and saved project/profile reads passed. Owner/project/profile hashes remained unchanged and the pre-reboot session was invalidated. |

Wrong target/schema/signature, unsupported host/privilege, failed acquisition,
changed identities, unsafe paths/modes/links and conflicting state retain their
automated fixture coverage. The single real guest does not establish support for
other architectures or operating systems. No fixture success substitutes for
the real packet verification above.

## Repeating the checks

Use a fresh, explicitly authorized disposable Ubuntu host and the
[documented installer commands](bundle-installation.md#plan-install-enroll-start).
Never run the storage or interruption experiments on an existing installation.
Capture a file inventory before and after retry: SHA-256, inode, UID/GID, mode
and link count of the installed outputs and receipt. Compare owner and saved
project/profile bytes privately; do not publish their contents or password hashes.
For the interruption case, stop only the test process after a completed production
write, then use the ordinary CLI to resume. Preserve unknown partial state if the
receipt or temporary-link boundary is interrupted instead.

For an isolated local TLS rehearsal, use the `.localhost` hostname above. Caddy
issued a certificate with `issuer=local`. Only the **public** root certificate
was copied out of the guest. SSH and HTTPS forwarding bound to host loopback;
HTTPS forwarded host port 2443 to guest 443. With that public certificate saved
as `test-root.crt`, this preserves the configured hostname, SNI and HTTPS origin:

```sh
curl --cacert test-root.crt \
  --connect-to platform.installer.localhost:443:127.0.0.1:2443 \
  --head https://platform.installer.localhost/projects
```

Expect 303 with `Location: /sign-in`. Do not disable certificate validation or
install the fixture CA into the host's global trust store. Repeat HTTPS checks
after `systemctl restart` and `reboot`, comparing boot IDs and checking both units
with `systemctl is-active` and `systemctl is-enabled`.

## Limits and cleanup

This establishes privileged installation, deterministic completed-write recovery,
identical retries and restart/reboot behavior for the retained validation packet.
It does **not** establish public DNS, inbound internet reachability or public
ACME issuance/renewal. That last external TLS acceptance needs an authorized
disposable public host and hostname; paid infrastructure and DNS changes need
their applicable separate approval. Existing unrelated cloud nodes were not used.

The guest was powered off after testing and its loopback forwards were closed.
The original rehearsal disk was preserved. Combined local rehearsal storage stayed
below the existing 8 GiB cap. No cloud resources, public DNS, repository security
settings, credentials for production, tags, release assets or publication state
were changed. The packet remains an unpublished validation candidate with
`publication_eligible: false`; upgrades and image distribution remain #263/#264.

Repository checks passed: locked offline workspace build/tests, formatting,
Clippy with warnings denied, all 238 Python tests using the pinned verifier and
release-plz, all 101 Node tests, append-only candidate audit, CI workflow
actionlint and `git diff --check`. The public evidence omits guest keys, session
cookies, owner/data digests and raw terminal/journal output.
