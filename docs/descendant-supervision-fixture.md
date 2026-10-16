# Linux orphan-descendant cleanup evidence

[#213](https://github.com/devoption/devoption/issues/213) extends the
[leaf-process tests](process-supervision-fixtures.md) with one trusted parent/child
fixture. Its parent exits successfully while its TERM-ignoring child retains both
output pipes. A successful parent exit must not complete the attempt while that
child remains alive.

The Rust implementation is test-only and Linux-only. It launches a separate copy
of the test executable with one exact ignored helper test selected. Only this
isolated helper enables the Linux child-subreaper attribute, using the existing
rustix dependency. A private completion receipt prevents a mismatched test filter or zero-test run
from passing the outer test. The main parallel test runner's process-wide settings are not
changed. This requires no root, new account, package installation or privileged setup.

## Execution and cleanup

The helper launches the fixed checked-in Python fixture with a cleared environment,
only LANG, isolated/no-site/no-bytecode flags and a private temporary working
directory. The fixture creates exactly one child in the parent's process group.
The child ignores TERM, reports readiness, creates a private readiness marker and
keeps both output descriptors open. The parent exits zero after readiness. Neither
process accesses the network or executes repository code.

The helper observes parent exit with `waitid` and `WNOWAIT`. It deliberately leaves
the leader unreaped so the group ID cannot be recycled while group signals remain
possible. Both pipes must still be open at that point. The helper requests
cancellation through the existing supervision policy and sends TERM to the group.
After the five-second grace, it sends KILL, then stops issuing group signals.

As a subreaper it adopts the orphan. A bounded `waitpgid` loop must collect exactly
two statuses: the leader's zero exit and the descendant's SIGKILL termination.
The loop continues until ECHILD, rather than treating one successful wait as
complete cleanup. It then requires EOF on both bounded nonblocking pipes, checks
the captured readiness/stderr text and verifies an unrelated sentinel is unchanged.
No RunnerEvent, artifact or successful build result is issued.

Reads are limited to one 4 KiB chunk per stream per loop and the shared retained
log budget is 4096 bytes. The supervised limit is ten seconds; readiness/exit
observation initiates cancellation earlier. Fixtures have independent twelve-second
alarms. The helper has a fifteen-second observation bound, three-second reaping
bound and one-second drain bound; its outer launcher has a twenty-five-second
watchdog. Error unwinding attempts group kill before reaping; it never signals
again after reaping starts. These are cooperative test bounds, not protection from
kernel stalls or guaranteed cleanup after the helper itself is forcibly killed.

## Scope of the evidence

This verifies one trusted orphan that stays in the original group. It does not
prove cleanup of descendants that escape via setsid/setpgid, containment under
subreaper failure, arbitrary fork trees, or recovery after supervisor restart.
Process groups/subreapers are not a substitute for independently tested production
containment and admission control. Network/filesystem isolation and resource quotas
remain absent. The [execution trust contract](execution-trust.md) still applies.
The application has no runner integration from this test.

## Validation

Ubuntu CI runs the outer test as part of the ordinary workspace suite. The outer
test runs the ignored helper itself; a skipped helper in the main listing is
intentional. To run just this evidence on Linux:

```sh
cargo test -p devoption-builds --locked --offline orphan_pipe_holder_requires_kill_and_reaping_after_leader_success -- --nocapture
```

Do not invoke the ignored helper manually: without the private test marker it does
nothing. `/usr/bin/python3` is required, as for the leaf fixtures. macOS does not
compile or execute this Linux subreaper test; local macOS checks cover the existing
leaf tests and remaining workspace. Linux CI must pass before claiming this new
cleanup behavior is verified. See [workspace checks](development.md#checks).
