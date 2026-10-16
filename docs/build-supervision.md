# In-memory build supervision decisions

[#204](https://github.com/devoption/devoption/issues/204) adds
`devoption_builds::supervision::Supervision`, combining elapsed-time, cancellation
and [bounded output](build-output.md) observations. This is synchronous decision
logic for a future runner, not process execution or OS enforcement. There is no
production caller, background timer, signal delivery, cleanup proof, job transition,
approval operation, persistence or UI.

## Timing and decisions

`Supervision::for_plan(&BuildPlan)` accepts only initial-policy elapsed limits of
1–60 seconds and combined output limits of 1–1,048,576 bytes. It owns the output
collector and records a private monotonic `Instant` at construction. The public API
accepts neither a clock nor an elapsed-time override. Other plan/context fields
remain outside this check; construction grants no execution authority.

A future runner must create exactly one supervision window at admission, retain
it through execution and artifact handoff, and never recreate it to extend a job.
Constructing one at queue time would incorrectly charge waiting time. Association
with the exact job and approved plan remains the future adapter's responsibility.

`poll()`, `cancel()` and `append(Stream, bytes)` return:

| Decision | Required future runner action |
| --- | --- |
| Continue | No stop condition observed yet; not evidence of successful execution |
| Terminate(reason) | Stop the complete job and begin/continue cleanup verification |
| ForceKill(reason) | Force termination of remaining children and verify cleanup |

Reasons are fixed: `Cancelled`, `TimedOut` and `OutputLimit`. The first observed
stop reason is retained. Before processing each new event, elapsed time is checked;
if the deadline has already arrived and no earlier stop exists, timeout wins over
a new cancellation or output-limit observation. Calls require `&mut self`; a future
multi-reader runner must serialize observations. This is observation order, not
proof of when external events actually happened.

Timeout's five-second grace starts at the original execution deadline, even if
polling is delayed. Cancellation/output grace starts when that stop is first
observed. At the exact grace boundary the decision is ForceKill. Repeated polling,
cancellation or appended output cannot reset the grace or replace its reason.
There is no success, finished or cleanup-complete decision. ForceKill remains a
requirement, not confirmation that any child was killed.

## Output and integration limits

The owned collector shares one budget between stdout/stderr. Exact fill triggers
OutputLimit even if no discarded byte has been observed. Late output after a stop
can still be retained within the remaining original budget, but cannot clear a
stop or change its reason. `output()` exposes only immutable collector access for
escaped text and cap/truncation metadata; there is no raw output Debug interface.
The [output guide](build-output.md) defines rendering, allocation and redaction limits.

There is no autonomous wakeup or hard realtime guarantee. Decisions only happen
when called, using the time observed at entry; copying output and caller scheduling
can delay the next observation. A future runner must enforce the
[execution trust contract](execution-trust.md#environment-resources-and-output)
with timely polling, bounded pipe readers, actual process-group/cgroup termination,
child reaping and independent OS limits. It must continue the deadline through
artifact handoff and must not admit another job until cleanup is verified.
Dropping this object only releases memory; it stops no process. The policy neither
restores authority after restart nor proves filesystem/network isolation.

## Validation

Run `cargo test -p devoption-builds --locked --offline supervision` and the
[workspace checks](development.md#checks). Module-private synthetic-time tests
cover exact execution/grace boundaries, delayed first polls, repeated observations,
sticky cancellation/output reasons, timeout precedence, late bounded output, empty
chunks and policy-limit rejection. Tests do not sleep, launch workloads or signal
processes. Actual termination and cleanup still require separate integration tests.
