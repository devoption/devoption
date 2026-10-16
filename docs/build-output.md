# Bounded in-memory build output

[#202](https://github.com/devoption/devoption/issues/202) adds
`devoption_builds::output::BuildOutput`, a collector for caller-supplied stdout and
stderr bytes. It is a prerequisite of [the build workflow](build-jobs.md), with no
production caller, process readers, persistence, HTTP/UI route or job-state changes.
Supplied output is not proof that a build ran or succeeded.

## Collection and bounds

`BuildOutput::for_plan(&BuildPlan)` uses the plan's explicit `limits.log_bytes`.
It rejects zero and values above 1,048,576 bytes, matching the initial
[execution policy](execution-trust.md#environment-resources-and-output). No fallback
or inherited setting is used. The collector does not approve or validate the rest
of the plan and does not establish job identity.

`append(Stream, &[u8])` retains only the prefix that fits the remaining combined
stdout/stderr budget. Bytes, not characters or rendered output, consume the budget.
The returned `Append` reports retained bytes and whether the limit is reached.
After the limit is reached, further input cannot grow storage. A nonempty discarded
suffix sets the sticky `truncated()` flag. Empty chunks do not clear it or mean EOF.
No unbounded count of discarded bytes or chunk metadata is accumulated.

Each stream reserves one fixed buffer of the configured limit, at most 2 MiB
combined capacity, but combined retained bytes never exceed 1 MiB. Appending copies
at most the remaining budget and never scans a discarded suffix. The caller owns
its input allocation; this API does not bound a future pipe reader's buffers.

Filling the budget, even exactly, returns `limit_reached = true`. A future runner
must stop the complete job and verify cleanup under the trust contract. Exact fill
can therefore request stopping while `truncated()` is still false: no discarded
byte has been observed yet. Neither flag proves exit, EOF, termination or success.
No automatic stop or runner event exists in this module. Integration must not turn
a cap-limited capture into successful build evidence.

## Safe display

`escaped_text(Stream)` renders the currently retained bytes as escaped HTML text
for a preformatted text node. Streams preserve their individual byte order; there
is no claim about chronology between streams. Decode happens on rendering so
UTF-8 sequences split across chunks of the same stream remain intact. Malformed
or incomplete sequences become U+FFFD. An incomplete live suffix can change from a
replacement character to the complete character after a later append.

The renderer escapes `&`, `<`, `>`, double quotes and apostrophes. It preserves
newline and tab; other Unicode control characters, including ESC, CR, backspace,
DEL and C1 controls, become visible `\u{hex}` text. Unicode bidi formatting marks
U+061C, U+200E/F, U+202A–E and U+2066–9 are also escaped. Literal backslashes are
doubled so a printed escape cannot impersonate a transformed control character.
ANSI/OSC sequences are displayed as inert text, not interpreted or stripped.
Ordinary Unicode remains unchanged; this is not a general Unicode spoof detector.

Rendered text is at most six times the retained byte count per stream (at most
6 MiB across both). Each call reserves that bounded output capacity; lossy UTF-8
decoding can additionally allocate up to three times that stream's retained bytes.
Callers must avoid retaining an unbounded history of rendered snapshots. Returned
text is not safe for HTML attributes, script contexts or terminal interpretation.
A future UI must display the cap/truncation status separately and use this content
in the documented text context; this slice adds no UI or truncation banner.

## Integration limits and validation

There is no raw-byte accessor or Debug implementation for the collector. Output
can still contain secrets or private paths: escaping is not redaction. Future
persistence and delivery must enforce owner access, private permissions and bounded
retention. Drop releases memory; it does not securely erase bytes.

The separate [supervision policy](build-supervision.md) owns this collector and
combines cap notifications with elapsed-time/cancellation decisions. It does not
perform termination or prove cleanup.

A future runner must serialize collection across its stdout/stderr readers,
associate the collector with the exact job, bound reader buffers, enforce time and
resource caps, and stop on the limit notification. Log collection alone does not
implement any of those controls or durable interrupted-state recovery.

Run `cargo test -p devoption-builds --locked --offline output` and the
[workspace checks](development.md#checks). Tests use only supplied byte slices:
combined/exact/overflow limits, sticky truncation, independent streams, every split
of a multibyte fixture, malformed/truncated UTF-8, HTML/ANSI/control/bidi escaping,
all byte values and the maximum retained size. No test launches a workload.
