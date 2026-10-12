"""Statically verify the user-unit example in a temporary home, without starting it."""

import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile


if sys.platform != "linux":
    sys.exit("systemd validation requires Linux; no service was installed or started")
if len(sys.argv) != 2:
    sys.exit("usage: python3 scripts/check_systemd_unit.py path/to/devoption")

binary = Path(sys.argv[1]).resolve(strict=True)
source = Path(__file__).resolve().parents[1] / "examples/systemd/devoption.service"
with tempfile.TemporaryDirectory(prefix="devoption-unit-") as directory:
    root = Path(directory).resolve()
    installed = root / ".local/lib/devoption/devoption"
    installed.parent.mkdir(parents=True, mode=0o700)
    shutil.copyfile(binary, installed)
    installed.chmod(0o700)
    data = root / ".local/share/devoption"
    data.mkdir(parents=True, mode=0o700)
    unit = root / "devoption.service"
    # Expand the documented home specifier to this disposable fixture only.
    content = source.read_text().replace("%h", str(root))
    unit.write_text(content)
    environment = os.environ | {
        "XDG_CONFIG_HOME": str(root / ".config"),
        "XDG_DATA_HOME": str(root / ".local/share"),
        "XDG_RUNTIME_DIR": str(root / "run"),
    }
    (root / "run").mkdir(mode=0o700)
    subprocess.run(
        ["systemd-analyze", "--user", "--man=no", "--recursive-errors=no", "verify", str(unit)],
        env=environment, check=True, timeout=30,
    )
    preflight = next(line.removeprefix("ExecStartPre=") for line in content.splitlines()
                     if line.startswith("ExecStartPre="))
    command = shlex.split(preflight)
    # The example must not start an unconfigured public preview after data loss.
    for payload in [None, b"", b"nonempty fixture; runtime separately validates format"]:
        if payload is not None:
            (data / "owner.bin").write_bytes(payload)
            (data / "owner.bin").chmod(0o600)
        result = subprocess.run(command, timeout=5)
        assert (result.returncode == 0) == bool(payload)

print("User unit: static verification and missing/empty owner preflight passed; no service started")
