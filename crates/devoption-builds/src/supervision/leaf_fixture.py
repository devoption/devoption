"""Trusted leaf-process fixtures only; never spawn children or access the network."""
import os
import signal
import sys
import time

mode = sys.argv[1]
assert mode in {"success", "failure", "wait", "ignore-term", "flood"}
assert "PATH" not in os.environ and "HOME" not in os.environ
assert os.getpgrp() == os.getpid()
# Independent last-resort bound if the Rust test fails before cleanup.
signal.alarm(12)
signal.signal(signal.SIGTERM, signal.SIG_DFL)
if mode == "ignore-term":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
print("READY", flush=True)
if mode == "success":
    print("<script>\x1b[31mfixture & output", flush=True)
    print("fixture stderr", file=sys.stderr, flush=True)
    sys.exit(0)
if mode == "failure":
    print("expected failure", file=sys.stderr, flush=True)
    sys.exit(7)
if mode == "flood":
    while True:
        os.write(1, b"x" * 8192)
        os.write(2, b"y" * 8192)
while True:
    time.sleep(0.01)
