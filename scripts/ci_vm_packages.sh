#!/usr/bin/env bash
# Only for GitHub-hosted disposable runners and the private installer test VM.
set -euo pipefail
if [[ "${DEVOPTION_DISPOSABLE_VM_PACKAGES:-}" != 1 ]] || [[ "$(id -u)" != 0 ]]; then
  echo 'VM package setup requires explicit disposable-host opt-in and root.' >&2
  exit 1
fi
. /etc/os-release
if [[ "$ID" != ubuntu || "${VERSION_CODENAME:-}" != noble ]]; then
  echo 'VM package setup requires Ubuntu 24.04.' >&2
  exit 1
fi
case "${1:-}" in
  vm-tools)
    packages=(qemu-system-x86 qemu-utils xorriso)
    if [[ "${2:-}" == apps && ! -x /usr/sbin/sshd ]]; then
      packages+=(openssh-server)
    fi
    ;;
  installer-guest) packages=(git sudo build-essential) ;;
  installer-guest-download)
    # libsystemd0 is part of the tool closure. Include matching service packages
    # so a repository update cannot strand the pinned image's exact-version deps.
    packages=(git sudo build-essential systemd systemd-sysv systemd-resolved
      systemd-timesyncd udev libpam-systemd)
    ;;
  installer-guest-offline)
    cd -- "${2:?Provide the prepared guest package directory.}"
    sha256sum --check --strict SHA256SUMS
    shopt -s nullglob
    # APT's no-download path must find complete archives in its own cache;
    # keep that cache writable inside the disposable VM, with absolute paths.
    packages=("$PWD"/*.deb)
    if [[ ${#packages[@]} == 0 ]]; then echo 'No prepared guest packages.' >&2; exit 1; fi
    timeout --kill-after=10s 150s env DEBIAN_FRONTEND=noninteractive apt-get \
      -o Dir::Etc::sourcelist=/dev/null -o 'Dir::Etc::sourceparts=-' \
      -o "Dir::Cache::archives=$PWD" \
      --no-download --no-remove install -y --no-install-recommends "${packages[@]}"
    exit
    ;;
  *) echo 'Select vm-tools, installer-guest, installer-guest-download or installer-guest-offline.' >&2; exit 1 ;;
esac
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
# Select only the repositories needed by these fixtures, without rewriting the
# host's source configuration. Keep Ubuntu's archive signature verification.
cat > "$scratch/ubuntu.list" <<'SOURCES'
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://archive.ubuntu.com/ubuntu noble main universe
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://archive.ubuntu.com/ubuntu noble-updates main universe
deb [signed-by=/usr/share/keyrings/ubuntu-archive-keyring.gpg] https://security.ubuntu.com/ubuntu noble-security main universe
SOURCES
# The apt sandbox user must be able to read the temporary source list.
chmod 755 "$scratch"
chmod 644 "$scratch/ubuntu.list"
options=(-o "Dir::Etc::sourcelist=$scratch/ubuntu.list" -o 'Dir::Etc::sourceparts=-'
  -o APT::Get::List-Cleanup=0 -o Acquire::Languages=none -o Acquire::Retries=2
  -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15 -o DPkg::Lock::Timeout=30)
timeout --kill-after=10s 120s apt-get "${options[@]}" -o APT::Update::Error-Mode=any update
install_options=()
if [[ "$1" == installer-guest-download ]]; then
  destination="${2:?Provide a new absolute guest package directory.}"
  [[ "$destination" == /* ]]
  mkdir -- "$destination"
  chmod 755 "$destination"
  # Resolve the full dependency closure, even when tools already exist on the
  # runner. Neither dpkg status nor apt's auto-install state may change on host.
  touch "$scratch/status"
  options+=(-o "Dir::State::status=$scratch/status"
    -o "Dir::State::extended_states=$scratch/extended_states"
    -o "Dir::Cache::archives=$destination")
  install_options=(--download-only)
fi
timeout --kill-after=10s 150s env DEBIAN_FRONTEND=noninteractive apt-get "${options[@]}" "${install_options[@]}" --no-remove install -y --no-install-recommends "${packages[@]}"
