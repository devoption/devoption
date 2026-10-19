"""Dedicated Ubuntu/systemd adapter for the verified bundle installer."""

import grp
import errno
import http.client
import os
from pathlib import Path
import platform
import pwd
import shutil
import socket
import subprocess
import time

ROOT_PATH = "/usr/sbin:/usr/bin:/sbin:/bin"


class Host:
    root_uid = 0

    def run(self, *args, interactive=False):
        tool = shutil.which(args[0], path=ROOT_PATH)
        if tool is None:
            raise ValueError("Required host command is unavailable: " + args[0])
        environment = {"PATH": ROOT_PATH, "LANG": "C.UTF-8"}
        return subprocess.run([tool, *args[1:]], env=environment, check=True,
                              capture_output=not interactive, timeout=None if interactive else 120)

    def preflight(self, root, required_bytes):
        if (os.geteuid() != 0 or platform.system() != "Linux" or platform.machine() != "x86_64" or
                platform.freedesktop_os_release().get("ID") != "ubuntu" or
                platform.freedesktop_os_release().get("VERSION_ID") != "24.04" or
                not Path("/run/systemd/system").is_dir() or not Path("/sys/fs/cgroup/cgroup.controllers").is_file()):
            raise ValueError("Use root on a dedicated Ubuntu 24.04 amd64 systemd/cgroup-v2 host.")
        for tool in ("git", "sudo", "useradd", "runuser", "systemctl", "systemd-analyze", "getent"):
            path = shutil.which(tool, path=ROOT_PATH)
            if path is None:
                raise ValueError("Install the documented Ubuntu prerequisite: " + tool)
            info = Path(path).resolve().stat()
            if info.st_uid != 0 or info.st_mode & 0o022:
                raise ValueError("Host commands must be administrator-controlled.")
        # These fixed paths can be on different filesystems. Require enough free
        # bytes on each, conservatively allowing staging plus installed content.
        for directory in ("/opt", "/var/lib", "/etc"):
            if shutil.disk_usage(directory).free < required_bytes:
                raise ValueError("Insufficient disk space on " + directory)
        self.run("systemctl", "show", "--property=Version", "--value")

    def account(self, name):
        try:
            account = pwd.getpwnam(name)
        except KeyError:
            if name != "root":
                try:
                    grp.getgrnam(name)
                except KeyError:
                    return None
                raise ValueError("Conflicting pre-existing service group: " + name)
            raise
        return account.pw_uid, account.pw_gid

    def check_account(self, name, home):
        if self.account(name) is None:
            return
        account = pwd.getpwnam(name)
        group = grp.getgrnam(name)
        if (account.pw_uid == 0 or account.pw_gid == 0 or group.gr_gid != account.pw_gid or
                account.pw_dir != home or account.pw_shell != "/usr/sbin/nologin" or group.gr_mem):
            raise ValueError("Existing service account does not match the dedicated installation: " + name)
        if os.getgrouplist(name, account.pw_gid) != [account.pw_gid]:
            raise ValueError("Service account has unexpected supplementary groups.")

    def create_account(self, name, home):
        if self.account(name) is None:
            self.run("useradd", "--system", "--user-group", "--no-create-home", "--home-dir", home,
                     "--shell", "/usr/sbin/nologin", name)
        self.check_account(name, home)

    def own(self, path, uid, gid):
        os.chown(path, uid, gid, follow_symlinks=False)

    def sync(self, directory):
        descriptor = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)

    def free_port(self, port):
        for family, address in ((socket.AF_INET, "0.0.0.0"), (socket.AF_INET6, "::")):
            if family == socket.AF_INET6 and not socket.has_ipv6:
                continue
            try:
                listener = socket.socket(family, socket.SOCK_STREAM)
            except OSError as error:
                if family == socket.AF_INET6 and error.errno in (errno.EAFNOSUPPORT, errno.EPROTONOSUPPORT):
                    continue
                raise
            with listener:
                if family == socket.AF_INET6:
                    listener.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                try:
                    listener.bind((address, port))
                except OSError as error:
                    if family == socket.AF_INET6 and error.errno == errno.EADDRNOTAVAIL:
                        continue  # An explicitly IPv4-only Ubuntu host is supported.
                    raise ValueError(f"Required TCP port {port} is unavailable; do not stop unrelated listeners.") from error

    def active(self, service):
        result = subprocess.run(["/usr/bin/systemctl", "is-active", "--quiet", service],
                                capture_output=True, timeout=10, check=False)
        if result.returncode not in (0, 3, 4):
            raise ValueError("Cannot inspect system service state.")
        return result.returncode == 0

    def validate(self, binary, caddy, hostname):
        self.run("systemd-analyze", "--man=no", "--recursive-errors=no", "verify",
                 "/etc/systemd/system/devoption-public.service", "/etc/systemd/system/caddy-devoption.service")
        # Execute only the already pinned/verified proxy, using its service UID.
        self.run("runuser", "-u", "caddy", "--", "/usr/bin/env", "XDG_DATA_HOME=/var/lib/caddy/data",
                 "XDG_CONFIG_HOME=/var/lib/caddy/config", caddy, "adapt", "--config", "/etc/caddy/Caddyfile",
                 "--adapter", "caddyfile", "--validate")

    def reload(self):
        self.run("systemctl", "daemon-reload")

    def enroll(self, binary):
        if not os.isatty(0) or not os.isatty(1):
            raise ValueError("Use an interactive SSH terminal for owner enrollment; no password argument or environment is supported.")
        self.run("runuser", "-u", "devoption", "--", binary, "setup", "--data-dir", "/var/lib/devoption/data", interactive=True)

    def start(self):
        self.run("systemctl", "enable", "--now", "devoption-public.service", "caddy-devoption.service")

    def healthy(self, hostname):
        # Local private-hop readiness does not establish public DNS/ACME. The
        # external HTTPS check remains mandatory in the operator instructions.
        for _ in range(20):
            try:
                connection = http.client.HTTPConnection("127.0.0.1", 3000, timeout=2)
                try:
                    connection.request("GET", "/projects", headers={"Host": hostname})
                    response = connection.getresponse()
                    if response.status == 303 and response.getheader("Location") == "/sign-in":
                        return
                finally:
                    connection.close()
            except OSError:
                pass
            time.sleep(0.5)
        raise ValueError("Services were enabled but the private platform is not ready; inspect status/journal without deleting data.")
