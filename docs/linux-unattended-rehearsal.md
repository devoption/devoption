# Ubuntu unattended-startup rehearsal — 2026-10-12

The bounded follow-up under [#94](https://github.com/devoption/devoption/issues/94)
verified startup before login and continued service after logout in the existing
[#92 guest](linux-rehearsal.md). The owner explicitly approved enabling lingering
for its fixture account, testing, and restoring the original policy afterward.

## Scope and method

The guest, fixture owner, restored data, backed-up binary, and enabled user unit
were reused. The binary was still built from public commit
[`da34de8beb239c76ea78509c82fd5a0d2527c429`](https://github.com/devoption/devoption/commit/da34de8beb239c76ea78509c82fd5a0d2527c429).
Subsequent main commit
[`a864c519989d7cc2c3ab45b4aabe2f4bab6e8e75`](https://github.com/devoption/devoption/commit/a864c519989d7cc2c3ab45b4aabe2f4bab6e8e75)
changed documentation and the ignore rule only. This was not another clean source
installation. Ubuntu 24.04.5 x86_64, systemd 255, QEMU TCG, Chrome, and the private
same-port SSH transport were unchanged from the first rehearsal.

The baseline was `Linger=no`. The approved policy change inside this disposable
guest was:

```sh
sudo loginctl enable-linger rehearsal
loginctl show-user rehearsal -p Linger
```

The result was `Linger=yes`. Lingering starts that account's user manager at boot
and retains it after logout; it applies to all enabled user services for the
account. See [systemd's loginctl documentation](https://github.com/systemd/systemd/blob/v255/man/loginctl.xml).
DevOption continued to run as UID `1000`; its application unit was not changed.

A temporary user unit, ordered after `devoption.service` and enabled under
`default.target`, ran an unprivileged Python probe with a 120-second service
timeout. It recorded success only when all of these were true:

- No login sessions existed, and the fixture account's session list was empty.
- DevOption was active with a nonzero PID owned by UID `1000`.
- That PID listened only on `127.0.0.1:13000` for the application's port.
- An unauthenticated request to `/projects` returned `303` to `/sign-in`.

The probe recorded the boot ID, service PID, and monotonic timestamps in a private
guest file. SSH and forwarding stayed disconnected during the observation
windows. After reconnecting, the file was compared with the current boot/PID and
the login manager's journal. For the reboot check, the readiness timestamp had
to precede the **first login of that boot**, not merely the command that retrieved
the evidence. The probe did not read passwords or owner-record contents.

## Measured results

| Check | Observed result |
| --- | --- |
| Final logout | Readiness with zero sessions at boot-relative time `205.961092 s`; next login at `269.088127 s`; the same DevOption PID remained active across the interval |
| Browser after logout | The existing cookie still authorized the protected page (`200`) and session recovery (`204`), consistent with the process remaining alive |
| Reboot | Boot ID changed; service started at `19.881495 s`; the probe confirmed readiness with zero sessions at `20.883036 s`; first login was later, at `48.887488 s` |
| Browser after reboot | The old cookie was rejected (`303` redirect and `401` session recovery); the existing fixture password signed in (`204`) and the protected page loaded (`200`) |
| Sign-out | Returned `204`; protected navigation redirected and session recovery returned `401` |
| Deliberate stop | Stayed inactive with no PID or listener for 12 seconds, exceeding the configured 5-second failure restart delay; explicit start restored HTTP `303` readiness |
| State integrity | Owner-record and application-unit hashes matched the baseline after testing |

These timings describe this emulated run; they are not performance guarantees.

## Restoration and containment

Inside the guest, the temporary probe was disabled and stopped, and its user unit,
executable and phase file were removed. After a user-manager reload, its unit
reported `LoadState=not-found`. The original policy was restored:

```sh
sudo loginctl disable-linger rehearsal
loginctl show-user rehearsal -p Linger
```

The result was `Linger=no`. DevOption was explicitly stopped and its listener was
absent before guest poweroff. Its original enabled unit, owner, restored data,
backup and earlier evidence were retained. The VM, browser, SSH tunnel and disk
monitor all exited. The separate existing VM and persistent development preview
were left intact.

All new host artifacts are under `.local/vm-rehearsal/issue94/`, apart from the
existing contained guest overlay, browser profile, cache/temp paths and runtime
PID/control files that were reused. Earlier rehearsal files were preserved. The
full ignored `.local/vm-rehearsal/` directory, including retained preview-build files, measured
about **1.85 GiB** before the test, at its observed peak, and after shutdown. The
existing **4 GiB** budget, **20 GiB** free-space floor, and one-second allocation
monitor remained in effect during the guest run; approximately **256 GiB** stayed
free. No additional image, VM, account or credential was created.

The [original stop and cleanup instructions](linux-rehearsal.md#containment-disk-budget-and-cleanup)
still apply to the containing folder. Cleanup removes its retained guest state
and fixture credentials too; it must not target another VM or directory.

## Limits

This validates optional lingering in one existing disposable Ubuntu guest. It
does not enable it by default, prove unattended operation on another host, change
SSH access requirements, or establish public hosting or availability while the
host is off. The Mac development preview still requires the Mac to be awake and
its user logged in. No system-level DevOption unit, root-run application, SSH/PAM/
polkit/firewall policy change, or project/build/deploy workflow was introduced.
The manual guest rehearsal is separate from the repository's CI fixtures.
