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
  vm-tools) packages=(qemu-system-x86 qemu-utils xorriso) ;;
  installer-guest) packages=(git sudo build-essential) ;;
  *) echo 'Select vm-tools or installer-guest.' >&2; exit 1 ;;
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
timeout --kill-after=10s 150s env DEBIAN_FRONTEND=noninteractive apt-get "${options[@]}" install -y --no-install-recommends "${packages[@]}"
