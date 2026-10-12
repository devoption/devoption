# Ubuntu runtime rehearsal — 2026-10-12

An isolated, disposable Ubuntu guest passed the current runtime installation and
recovery checks under [#92](https://github.com/devoption/devoption/issues/92).
This is evidence for the [private installation guide](private-linux-install.md),
not completion of the project/build/deploy workflow in
[#20](https://github.com/devoption/devoption/issues/20).

## Environment and source

| Item | Observed configuration |
| --- | --- |
| Source | [`da34de8beb239c76ea78509c82fd5a0d2527c429`](https://github.com/devoption/devoption/commit/da34de8beb239c76ea78509c82fd5a0d2527c429), public repository clone |
| Guest | Ubuntu 24.04.5 LTS, x86_64, Linux 6.8.0-146-generic, systemd 255 |
| Image | Official [minimal amd64 release 20261008](https://cloud-images.ubuntu.com/minimal/releases/noble/release-20261008/), 264,503,296 bytes |
| Image SHA-256 | `2f119f50bf29fe40e73b69ae2a736e8210d8146796f036e0324315b6b3a777df`, matched the official HTTPS checksum file |
| VM | QEMU 11.1.2, TCG emulation on an ARM64 Mac, 2 CPUs, 2 GiB RAM |
| Disk | One read-only base image and one sparse 6 GiB virtual overlay; no preallocation or host directory sharing |
| Build | Rust 1.99.0; locked dependency fetch, then offline release build; compilation took 2m 34s under emulation |
| Browser | Chrome 153.0.8010.48 with a separate profile |
| Transport | Host `127.0.0.1:2222` forwards only guest SSH; SSH forwards host `127.0.0.1:13000` to guest `127.0.0.1:13000` |

Guest prerequisites were installed using Ubuntu's package repositories:
`build-essential`, `curl`, `git`, and `ca-certificates`. Rust was installed through
the official rustup script with a minimal profile and the pinned toolchain.
Package/toolchain setup and `cargo fetch --locked` required internet access;
`cargo build --release --locked --offline -j 2` completed afterward. The cloud
image already contained SSH, Python and the systemd user-session prerequisites.

Only this guest received a temporary OS account, SSH keys and a fixture owner.
The host key was pinned before connecting; SSH password and agent authentication
were not used. Owner setup ran over an SSH PTY with hidden input. No credentials
were passed through command arguments, copied from another installation, or
included in the public evidence. No private prototype code or disk overlay was
used. Existing VMs and the separate development preview were left intact.

## Results

| Check | Result |
| --- | --- |
| Fresh installation | The guide's directory preflight and install block completed; data directory `0700`, owner record `0600`, both owned by the unprivileged guest account |
| CLI owner setup | Prompt and exact confirmation completed over SSH; password was not echoed |
| User service | Example unit passed verification and ran as the guest user; only the documented port override to `13000` was needed |
| Anonymous browser access | Protected page redirected to Sign in (`303`); session recovery returned `401` |
| Browser sign-in | Returned `204`, cleared the password field, and allowed protected pages (`200`) and session recovery (`204`) |
| Tunnel guards | Wrong Host port returned `400`; wrong Origin port returned `403` |
| Service restart | Unit restarted; the existing browser cookie no longer authorized reads; the same fixture password signed in again |
| Backup and restore | Service was stopped; owner record, binary and source revision were copied to a private backup; owner bytes matched a fresh `0700` restore with a `0600` record |
| Adopt restored state | Both unit data-path references and its binary path were updated; the backed-up binary ran against restored data; old session rejected, existing password accepted |
| Browser sign-out | Returned `204`; subsequent protected navigation redirected and session recovery returned `401` |
| Last OS logout | With `Linger=no`, the service stopped after all SSH sessions and test helper processes ended; the guest journal recorded the stop |
| Next OS login | The enabled user service started again; the old browser session was rejected |
| Guest reboot | Boot ID changed, owner record stayed byte-identical, and the enabled service started after SSH login; the old browser session was rejected and the existing password worked |
| End state | Guest, browser, SSH tunnel, test keeper and disk monitor stopped |

The initial cloud-init seed was not detected as an emulated CD-ROM; attaching the
same small ISO as a read-only virtio disk enabled NoCloud setup. The first logout
probe also found a surviving non-PTY sleep helper, so that was an incomplete
logout. After removing only the helper and repeating with no surviving sessions,
the journal confirmed the stop and next-login start. Neither adjustment changed
DevOption or enabled lingering.

## Containment, disk budget and cleanup

All local rehearsal-owned artifacts live in the gitignored
`.local/vm-rehearsal/` folder inside the checkout: image/downloads, overlay,
firmware variables, seed/configuration, temporary keys, fixture password, browser
profile, screenshots, logs, result records and helper scripts. Host cache/temp
paths were redirected there. Guest package and Rust caches live inside its
overlay. Existing host tools and the existing Playwright runtime were reused;
no browser or VM manager was installed on the host.

Free space was checked before creation: approximately **258 GiB**, with a
**20 GiB minimum free-space floor**. New artifacts had a **4 GiB allocation
budget**, monitored every second with 256 MiB of stop headroom. The observed peak
was approximately **1.72 GiB**, and the stopped rehearsal retained approximately
**1.72 GiB**. These are allocated host bytes measured with `du`, not the overlay's
6 GiB virtual capacity. About **257 GiB** remained free. Nothing from an existing
VM, model store, repository or build cache was deleted to make space.

On the workspace that performed this rehearsal, the ignored folder's `README.md`
describes its local controls and evidence. These helpers are not shipped with a
fresh repository clone. From that workspace's repository root:

```sh
python3 .local/vm-rehearsal/control.py status
python3 .local/vm-rehearsal/control.py stop
touch .local/vm-rehearsal/monitor.stop
```

After status shows no rehearsal processes and the monitor has exited, cleanup is
one directory removal:

```sh
rm -rf -- .local/vm-rehearsal
```

This removes the temporary credentials and guest state too. Do not remove any
other directory or reuse these fixture credentials. The folder remains local and
ignored; its private contents must not be added to commits or issue attachments.

## Limits

This was one controlled guest rehearsal, not a new installer or an automated CI
VM test. It does not establish physical-host performance, other Linux versions or
architectures, other browsers, public hosting, off-host disaster recovery, or
cross-version storage migration. Service operation after logout or before login
still depends on the host's user-manager policy: this guest had no lingering and
did not provide unattended service persistence after the final logout. Reboot
recovery was checked after a new SSH login. The unimplemented project, build,
artifact and deployment workflows remain outside this result.
