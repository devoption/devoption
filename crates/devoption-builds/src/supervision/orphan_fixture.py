"""Trusted two-process fixture: leader exits; child holds pipes and ignores TERM."""
import os
import signal
import subprocess
import sys
import time

signal.alarm(12)
assert "PATH" not in os.environ and "HOME" not in os.environ
if len(sys.argv) == 2 and sys.argv[1] == "child":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    print("CHILD_READY", flush=True)
    print("CHILD_STDERR", file=sys.stderr, flush=True)
    with open("child-ready", "x", encoding="ascii") as marker:
        marker.write(str(os.getpid()))
    while True:
        time.sleep(0.01)
assert len(sys.argv) == 1
assert os.getpgrp() == os.getpid()
subprocess.Popen([sys.executable, "-I", "-B", "-S", "-u", __file__, "child"],
                 env={"LANG": "C.UTF-8"}, stdin=subprocess.DEVNULL)
while not os.path.exists("child-ready"):
    time.sleep(0.01)
os._exit(0)
