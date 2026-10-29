"""Install a verified preview on a fresh dedicated Ubuntu host; never publish or upgrade."""

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import select
import stat
import subprocess
import tarfile
import tempfile
import time
import zipfile

from deployment_broker import hostname as valid_hostname
from release_bundle import CADDY_SHA512, CADDY_VERSION, MAX_BINARY, canonical, RUNTIME, file_mode
from release_candidates import load, verify_transport
from verify_bundle_provenance import manifest_from_archive, snapshot
from installer_host import Host

STATE = "/var/lib/devoption-installer"
PLATFORM = "/var/lib/devoption"
PROXY = "/var/lib/caddy"
MIN_FREE = 1024 * 1024 * 1024
PACKET_LIMIT = MAX_BINARY + 16 * 1024 * 1024


@contextmanager
def installation_lock(path, owner=0):
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "r+b") as lock:
        info = os.fstat(lock.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != owner or info.st_nlink != 1 or
                stat.S_IMODE(info.st_mode) != 0o600):
            raise ValueError("Unsafe installer lock.")
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


def verified_files(packet, record, gh, allow_validation):
    if not allow_validation:
        raise ValueError("This is an unpublished validation candidate; explicitly select --allow-validation-candidate.")
    # Snapshot before verification: never extract or execute a mutable caller path.
    raw = snapshot(packet, MAX_BINARY + 16 * 1024 * 1024)
    with tempfile.TemporaryDirectory(prefix="devoption-install-verify-") as temporary:
        immutable = Path(temporary) / "packet.zip"
        immutable.write_bytes(raw)
        verify_transport(immutable, record, gh)
    candidate = record["candidate"]
    with zipfile.ZipFile(io.BytesIO(raw)) as transport:
        archive = transport.read(candidate["archive"])
    manifest = manifest_from_archive(archive, candidate["source_commit"], candidate["platform_version"])
    metadata = manifest["release_metadata"]
    if (metadata["compatibility"]["installation"] not in ("fresh-only", "fresh-and-upgrade") or
            metadata["migration"]["required"]):
        raise ValueError("Installer supports fresh previews without migrations; use the upgrade contract for other versions.")
    # manifest_from_archive rejected unsafe paths/types, duplicates, wrong modes,
    # unexpected files and mismatched digests. Copy bytes ourselves; no extractall.
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as bundle:
        files = {member.name: bundle.extractfile(member).read() for member in bundle}
    return manifest, files


def proxy_binary(archive, manifest):
    proxy = manifest["external_requirements"]["proxy"]
    if proxy["version"] != CADDY_VERSION or proxy["sha512"] != CADDY_SHA512:
        raise ValueError("Unsupported proxy acquisition pin.")
    raw = snapshot(archive, MAX_BINARY)
    if hashlib.sha512(raw).hexdigest() != CADDY_SHA512:
        raise ValueError("Caddy archive differs from the pinned SHA-512.")
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as bundle:
        members = bundle.getmembers()
        matches = [m for m in members if m.name == "caddy"]
        if (len(members) > 64 or len(matches) != 1 or not matches[0].isfile() or
                not 64 <= matches[0].size <= MAX_BINARY):
            raise ValueError("Pinned Caddy archive has no bounded regular executable.")
        return bundle.extractfile(matches[0]).read()


def fetch_packet(asset_id, destination, gh):
    if not asset_id.isascii() or not asset_id.isdecimal() or not 0 < int(asset_id) < 10**20:
        raise ValueError("Supply one positive GitHub release asset ID.")
    with destination.open("xb") as stream:
        process = subprocess.Popen([str(gh), "api", "--hostname", "github.com", "--method", "GET",
                                    "repos/devoption/devoption/releases/assets/" + asset_id,
                                    "-H", "Accept: application/octet-stream"],
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        try:
            deadline, total = time.monotonic() + 120, 0
            while True:
                if time.monotonic() >= deadline:
                    raise ValueError("Release asset download exceeded its deadline.")
                if not select.select([process.stdout], [], [], 0.5)[0]:
                    continue
                chunk = os.read(process.stdout.fileno(), 1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > PACKET_LIMIT:
                    raise ValueError("Release asset download exceeds its size limit.")
                stream.write(chunk)
            if process.wait(timeout=max(0.1, deadline - time.monotonic())) != 0:
                raise ValueError("Release asset download failed; no installation changes were made.")
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            process.stdout.close()
    snapshot(destination, PACKET_LIMIT)


class Installer:
    """Root-owned resources are reconciled from verified bytes, never a saved command list.

    A different root/Host is for disposable fixtures and future image staging only;
    the operator CLI always selects / and the real host preflight.
    """

    def __init__(self, root, host, record, manifest, files, caddy, hostname):
        valid_hostname(hostname)
        if any(manifest[key] != record["candidate"][key] for key in ("platform_version", "source_commit")):
            raise ValueError("Verified installation manifest differs from the reserved identity.")
        self.root, self.host, self.record = root, host, record
        self.manifest, self.files, self.hostname = manifest, files, hostname
        self.version = record["candidate"]["platform_version"]
        self.release = "/opt/devoption/releases/" + self.version
        self.binary = self.release + "/bin/devoption"
        self.caddy = "/opt/devoption/proxy/caddy-" + CADDY_VERSION
        self.identity = {"schema_version": 1, "reservation": record, "hostname": hostname,
                         "platform_port": 3000, "proxy_sha512": CADDY_SHA512}
        self.output = {self.release + "/" + name: (data, file_mode(name))
                       for name, data in files.items()}
        unit = files["examples/systemd/devoption-public.service"].decode()
        unit = unit.replace("/opt/devoption/devoption", self.binary).replace("platform.example.com", hostname)
        proxy_unit = files["examples/systemd/caddy-devoption.service"].decode().replace("/usr/local/bin/caddy", self.caddy)
        self.output |= {self.caddy: (caddy, 0o755),
                        "/etc/systemd/system/devoption-public.service": (unit.encode(), 0o644),
                        "/etc/systemd/system/caddy-devoption.service": (proxy_unit.encode(), 0o644),
                        "/etc/caddy/Caddyfile": (files["examples/caddy/Caddyfile"].replace(b"platform.example.com", hostname.encode()), 0o644)}
        self.dirs = {"/opt/devoption": (0o755, "root"), "/etc/caddy": (0o755, "root"),
                     PLATFORM: (0o700, "devoption"), PLATFORM + "/data": (0o700, "devoption"),
                     PLATFORM + "/repositories": (0o700, "devoption"), PROXY: (0o700, "caddy"),
                     PROXY + "/data": (0o700, "caddy"), PROXY + "/config": (0o700, "caddy")}
        for name in self.output:
            parent = Path(name).parent
            while str(parent).startswith("/opt/devoption/"):
                self.dirs[str(parent)] = (0o755, "root")
                parent = parent.parent

    def runtime_probe(self):
        if self.manifest['schema_version'] == 3:
            self.host.runtime_probe(self.release + '/' + RUNTIME, self.version)

    def path(self, name):
        return self.root / name.lstrip("/")

    def safe(self, path):
        # All installation parents must be real, administrator-owned directories.
        # Writable persistent leaves are checked against their service identities.
        current = path.parent
        while current != self.root.parent:
            if current.exists() or current.is_symlink():
                info = current.lstat()
                if not stat.S_ISDIR(info.st_mode) or info.st_mode & 0o022:
                    raise ValueError("Unsafe installation parent: " + str(current))
                if current not in (self.path(p) for p, (_, who) in self.dirs.items() if who != "root"):
                    if info.st_uid != self.host.root_uid:
                        raise ValueError("Installation parent must be administrator-owned.")
            if current == self.root:
                break
            current = current.parent

    def existing_file(self, name, data, mode):
        path = self.path(name)
        self.safe(path)
        if path.exists() or path.is_symlink():
            info = path.lstat()
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != self.host.root_uid or
                    info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != mode or
                    snapshot(path, MAX_BINARY + 16 * 1024 * 1024) != data):
                raise ValueError("Existing installed file differs; never replace it: " + name)
            return True
        return False

    def receipt(self):
        path = self.path(STATE + "/installation.json")
        self.safe(path)
        if not path.exists() and not path.is_symlink():
            return False
        if snapshot(path, 16384) != canonical(self.identity):
            raise ValueError("Installation identity differs; preserve data and use the platform upgrade/configuration contract (#263).")
        self.existing_file(STATE + "/installation.json", canonical(self.identity), 0o600)
        return True

    def preflight(self):
        self.prepare_output()
        self.host.preflight(self.root, MIN_FREE + sum(len(data) for data, _ in self.output.values()) * 2)
        resumed = self.receipt()
        if not resumed:
            for path in (STATE, "/opt/devoption", PLATFORM, PROXY, "/etc/caddy",
                         "/etc/systemd/system/devoption-public.service", "/etc/systemd/system/caddy-devoption.service"):
                if self.path(path).exists() or self.path(path).is_symlink():
                    raise ValueError("Unmanaged or interrupted installation without its receipt; inspect before retrying: " + path)
            for account in ("devoption", "caddy"):
                if self.host.account(account) is not None:
                    raise ValueError("Existing service account conflicts with a fresh installation.")
        else:
            if self.path(STATE).stat().st_uid != self.host.root_uid or stat.S_IMODE(self.path(STATE).stat().st_mode) != 0o700:
                raise ValueError("Installer state must be private and administrator-owned.")
        # Check every existing output before making any change, including on retry.
        for name, (content, mode) in self.output.items():
            self.existing_file(name, content, mode)
        allowed = set(self.output) | set(self.dirs)
        for name in ("/opt/devoption", "/etc/caddy"):
            root = self.path(name)
            if root.is_dir() and not root.is_symlink():
                for parent, directories, files in os.walk(root, followlinks=False):
                    for leaf in directories + files:
                        actual = "/" + (Path(parent) / leaf).relative_to(self.root).as_posix()
                        if actual not in allowed:
                            raise ValueError("Unexpected file or directory in installed inventory: " + actual)
        for name, (mode, who) in self.dirs.items():
            path = self.path(name)
            self.safe(path)
            if path.exists() or path.is_symlink():
                account = self.host.account(who)
                info = path.lstat()
                if (account is None or not stat.S_ISDIR(info.st_mode) or info.st_uid != account[0] or
                        info.st_gid != account[1] or stat.S_IMODE(info.st_mode) != mode):
                    raise ValueError("Existing directory identity or mode differs: " + name)
        for who, home in (("devoption", PLATFORM), ("caddy", PROXY)):
            self.host.check_account(who, home)
        for port, service in ((3000, "devoption-public.service"), (80, "caddy-devoption.service"), (443, "caddy-devoption.service")):
            if not resumed or not self.host.active(service):
                self.host.free_port(port)
        missing = [name for name in self.output if not self.path(name).exists()]
        return {"schema_version": 1, "platform_version": self.version, "resumed": resumed,
                "missing_files": missing, "owner_enrolled": self.owner(),
                "publication_eligible": False, "upgrades_supported": False}

    def owner(self):
        path = self.path(PLATFORM + "/data/owner.bin")
        if not path.exists() and not path.is_symlink():
            return False
        info = path.lstat()
        account = self.host.account("devoption")
        if (account is None or not stat.S_ISREG(info.st_mode) or info.st_uid != account[0] or
                info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600 or info.st_size == 0):
            raise ValueError("Existing owner record is invalid; preserve it and inspect recovery instructions.")
        return True

    def write(self, name, content, mode):
        if self.existing_file(name, content, mode):
            return
        path = self.path(name)
        # Commit each file atomically, without replacing any existing destination.
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".install-", delete=False) as stream:
            temporary = Path(stream.name)
            try:
                os.fchmod(stream.fileno(), mode)
                stream.write(content)
                stream.flush()
                os.fsync(stream.fileno())
                os.link(temporary, path)
            finally:
                temporary.unlink()
        self.host.sync(path.parent)

    def install(self):
        plan = self.preflight()
        if not plan["resumed"]:
            self.path(STATE).mkdir(mode=0o700)
            self.write(STATE + "/installation.json", canonical(self.identity), 0o600)
        for who, home in (("devoption", PLATFORM), ("caddy", PROXY)):
            self.host.create_account(who, home)
        self.prepare_output()
        for name, (mode, who) in sorted(self.dirs.items(), key=lambda item: len(Path(item[0]).parts)):
            path = self.path(name)
            if not path.exists():
                path.mkdir(mode=mode)
                uid, gid = self.host.account(who)
                self.host.own(path, uid, gid)
                path.chmod(mode)
                self.host.sync(path.parent)
        for name, (content, mode) in self.output.items():
            self.write(name, content, mode)
        self.host.validate(self.binary, self.caddy, self.hostname)
        self.runtime_probe()
        self.host.reload()
        return self.preflight()

    def prepare_output(self):
        """Optional configuration rendering after service identities are known."""

    def ready(self):
        plan = self.preflight()
        if not plan["resumed"] or plan["missing_files"]:
            raise ValueError("Run install with the same verified inputs before enrollment or start.")
        return plan

    def enroll(self):
        self.ready()
        if not self.owner():
            self.host.enroll(self.binary)
        if not self.owner():
            raise ValueError("Owner enrollment did not finish; inspect existing state before retrying.")

    def start(self):
        self.ready()
        if not self.owner():
            raise ValueError("Enroll the owner interactively over SSH before starting services.")
        self.host.validate(self.binary, self.caddy, self.hostname)
        self.host.start()
        self.host.healthy(self.hostname)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("plan", "install", "enroll", "start", "status"))
    parser.add_argument("--reservation", type=Path, required=True, help="Trusted independently reviewed reservation, never a downloaded self-description")
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--packet", type=Path)
    source.add_argument("--asset-id")
    parser.add_argument("--gh", type=Path, required=True)
    parser.add_argument("--caddy-archive", type=Path, required=True)
    parser.add_argument("--hostname", required=True)
    parser.add_argument("--allow-validation-candidate", action="store_true")
    args = parser.parse_args()
    try:
        record = load(snapshot(args.reservation, 8192))
        with tempfile.TemporaryDirectory(prefix="devoption-install-input-") as directory:
            packet = args.packet or Path(directory) / "candidate.zip"
            if args.asset_id:
                fetch_packet(args.asset_id, packet, args.gh)
            manifest, files = verified_files(packet, record, args.gh, args.allow_validation_candidate)
            caddy = proxy_binary(args.caddy_archive, manifest)
            host = Host()
            installer = Installer(Path("/"), host, record, manifest, files, caddy, args.hostname)
            if args.mode in ("plan", "status"):
                print(json.dumps(installer.preflight(), indent=2))
            else:
                host.preflight(Path("/"), MIN_FREE)
                # Reject precreated links/foreign files even on a shared lock dir.
                with installation_lock("/run/lock/devoption-installer.lock"):
                    if args.mode == "install":
                        print(json.dumps(installer.install(), indent=2))
                    elif args.mode == "enroll":
                        installer.enroll()
                    else:
                        installer.start()
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError, tarfile.TarError, zipfile.BadZipFile) as error:
        parser.exit(1, f"Installation stopped: {error}\n")


if __name__ == "__main__":
    main()
