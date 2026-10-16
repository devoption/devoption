# Real leaf-process supervision evidence

[#211](https://github.com/devoption/devoption/issues/211) connects the
[supervision policy](build-supervision.md) and [output collector](build-output.md)
to real trusted subprocesses in tests. The implementation is compiled only under
`cfg(test)`. It is not a production executor, does not consume approval and cannot
issue a RunnerEvent or make application builds available.

## Fixed execution scope

Each attempt directly starts `/usr/bin/python3 -I -B -S -u` with the checked-in
[leaf fixture](../crates/devoption-builds/src/supervision/leaf_fixture.py) and one
fixed test mode. No shell, caller-selected executable, dependency download or
registered/user repository is executed. Python isolated mode disables user/PYTHON
configuration; site imports and bytecode writes are disabled. The environment is
cleared and only `LANG=C.UTF-8` is supplied. Python may initialize locale state;
this is not the production build environment. The fixture asserts that PATH and
HOME were not inherited and that its process-group ID equals its PID.

The working directory is a new disposable temporary tree. A sentinel file is
checked unchanged after each attempt. stdin is closed; stdout/stderr are pipes.
The fixture never spawns children, accesses the network or executes Cargo. Its
Python runtime and source are trusted host files, not a verified toolchain. The
build plan supplies only limits to the policy; the fixture command is deliberately
not represented as the plan's approved Cargo command.

## Controls exercised

The Rust test puts the child in a new process group and makes both output pipes
nonblocking. Each loop reads at most one 8 KiB chunk per stream, then checks
cancellation, elapsed time and child status. A continuously readable stream cannot
prevent polling the other stream or the policy. A readiness marker ensures the
cancellation/TERM-ignore cases are initialized before cancellation is requested.

Terminate sends SIGTERM once to the owned group. If the direct child is still
running after the original five-second grace, ForceKill sends SIGKILL once.
No group signal is sent after reaping the leader, since PID/group reuse cannot
then be excluded. Both pipes must reach EOF and the direct child must be reaped
before an outcome is returned. There is no reader thread left blocked on a pipe.

Tests assert:

- Real zero and exit-code-7 results, with both streams captured and markup/ANSI
  escaped. A Continue policy decision alone is not success: exit status is separate.
- A real one-second timeout and explicit cancellation, each ending with SIGTERM.
- A stdout/stderr flood stopped by the combined 1024-byte cap, with sticky truncation.
- A TERM-ignoring leaf ending with SIGKILL after at least five seconds of grace.
- Unchanged sentinel bytes and completed direct-child wait for each attempt.

On panic or I/O failure, a guard kills and waits for the direct leaf. The fixture
also installs a 12-second SIGALRM as an independent emergency bound; the test loop
fails after 15 seconds. These are ordinary OS scheduling bounds, not guarantees
against an uninterruptible kernel stall. Signal-specific assertions prevent an
emergency alarm exit from being mistaken for successful TERM/KILL supervision.

## What this does not prove

These leaf fixtures have no descendants. The separate
[Linux subreaper fixture](descendant-supervision-fixture.md) tests one orphan
that remains in its original group. Process groups are not cgroups or a sandbox:
children can leave a group, and reaping the leader does not prove all writers are
gone. Production must implement independently tested descendant containment,
termination and cleanup before artifact acceptance or another admission.

There is no CPU/memory/process/disk quota, network/filesystem isolation, private
runtime account, approval transaction, durable job/log storage, artifact handoff
or production single-job scheduler here. Multiple trusted tests may run concurrently.
The [execution trust contract](execution-trust.md) still requires all production
controls. This evidence must not be used to enable arbitrary command execution or
registered-repository builds. The separate [offline Cargo fixture](offline-fixture-build.md)
provides compilation evidence; these leaf tests do not build a service.

## Running the checks

On Unix with `/usr/bin/python3` available (macOS and Ubuntu CI):

```sh
cargo test -p devoption-builds --locked --offline supervision::process_tests -- --test-threads=1
```

The five tests launch six disposable leaf processes and take roughly seven seconds
when serialized. They are also included in the ordinary workspace test suite and
Ubuntu CI; no privileged setup or new dependency is required. Run the complete
[workspace checks](development.md#checks) for changes.
