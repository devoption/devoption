//! Real trusted leaf-process evidence, not a general runner or isolation boundary.
use super::*;
use rustix::{
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    process::{Pid, Signal, kill_process_group},
};
use std::{
    fs,
    io::{self, Read},
    os::{
        fd::AsFd,
        unix::process::{CommandExt, ExitStatusExt},
    },
    process::{Child, Command, ExitStatus, Stdio},
    thread,
};

// This guard owns only the direct leaf. No descendant-cleanup claim is made.
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        // Child::kill does not signal an already reaped PID. Fixture SIGALRM is
        // an independent bound; these fixtures never create descendants.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Outcome {
    status: ExitStatus,
    decision: Decision,
    output: BuildOutput,
    term_sent: bool,
    kill_sent: bool,
    elapsed: Duration,
}
fn nonblocking(pipe: &impl AsFd) {
    fcntl_setfl(pipe, fcntl_getfl(pipe).unwrap() | OFlags::NONBLOCK).unwrap();
}
fn read_once(pipe: &mut impl Read, stream: Stream, monitor: &mut Supervision) -> io::Result<bool> {
    let mut buffer = [0; 8192];
    match pipe.read(&mut buffer) {
        Ok(0) => Ok(true),
        Ok(count) => {
            monitor.append(stream, &buffer[..count]);
            Ok(false)
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}
fn run(mode: &str, seconds: u32, log_bytes: u32, cancel: bool) -> Outcome {
    let temporary = tempfile::tempdir().unwrap();
    let sentinel = temporary.path().join("unrelated-sentinel");
    fs::write(&sentinel, b"preserve me").unwrap();
    let mut plan = crate::review_context::tests::plan();
    plan.0.limits.elapsed_seconds = seconds;
    plan.0.limits.log_bytes = log_bytes;
    // The plan supplies limits only. This test does not execute its Cargo command
    // or pretend the Python fixture matches an approved build plan.
    let mut monitor = Supervision::for_plan(&plan).unwrap();
    let start = Instant::now();
    let child = Command::new("/usr/bin/python3")
        .args([
            "-I",
            "-B",
            "-S",
            "-u",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/supervision/leaf_fixture.py"
            ),
            mode,
        ])
        .env_clear()
        .env("LANG", "C.UTF-8")
        .current_dir(temporary.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .expect("trusted fixture requires /usr/bin/python3");
    let mut child = ChildGuard(child);
    let pid = Pid::from_raw(child.0.id() as i32).unwrap();
    assert!(pid.as_raw_nonzero().get() > 1);
    let mut stdout = child.0.stdout.take().unwrap();
    let mut stderr = child.0.stderr.take().unwrap();
    nonblocking(&stdout);
    nonblocking(&stderr);
    let (mut out_eof, mut err_eof) = (false, false);
    let (mut term_sent, mut kill_sent) = (false, false);
    let mut status = None;
    let mut cancellation_requested = false;
    loop {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "fixture cleanup exceeded emergency bound"
        );
        // One bounded read from each pipe per pass prevents a flood on one stream
        // from starving the other stream, cancellation, timing or process checks.
        if !out_eof {
            out_eof = read_once(&mut stdout, Stream::Stdout, &mut monitor).unwrap();
        }
        if !err_eof {
            err_eof = read_once(&mut stderr, Stream::Stderr, &mut monitor).unwrap();
        }
        if cancel
            && !cancellation_requested
            && monitor
                .output()
                .escaped_text(Stream::Stdout)
                .contains("READY")
        {
            monitor.cancel();
            cancellation_requested = true;
        }
        let decision = monitor.poll();
        if status.is_none() {
            status = child.0.try_wait().unwrap();
        }
        // Never signal a process group after reaping its leader: PID reuse cannot
        // be excluded then. Escaping descendants require stronger production control.
        if status.is_none() {
            let signal = match decision {
                Decision::Terminate(_) if !term_sent => {
                    term_sent = true;
                    Some(Signal::TERM)
                }
                Decision::ForceKill(_) if !kill_sent => {
                    kill_sent = true;
                    Some(Signal::KILL)
                }
                _ => None,
            };
            if let Some(signal) = signal {
                match kill_process_group(pid, signal) {
                    Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                    Err(error) => panic!("fixture signal failed: {error}"),
                }
            }
        }
        if let Some(status) = status
            && out_eof
            && err_eof
        {
            assert_eq!(child.0.wait().unwrap(), status);
            assert_eq!(fs::read(&sentinel).unwrap(), b"preserve me");
            return Outcome {
                status,
                decision,
                output: monitor.output,
                term_sent,
                kill_sent,
                elapsed: start.elapsed(),
            };
        }
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn real_leaf_success_and_nonzero_exit_capture_both_streams() {
    let success = run("success", 10, 4096, false);
    assert!(success.status.success());
    assert_eq!(success.decision, Decision::Continue);
    assert!(!success.term_sent && !success.kill_sent);
    assert!(
        success
            .output
            .escaped_text(Stream::Stdout)
            .contains("&lt;script&gt;\\u{1b}[31mfixture &amp; output")
    );
    assert!(
        success
            .output
            .escaped_text(Stream::Stderr)
            .contains("fixture stderr")
    );
    let failure = run("failure", 10, 4096, false);
    assert_eq!(failure.status.code(), Some(7));
    assert_eq!(failure.decision, Decision::Continue);
    assert!(
        failure
            .output
            .escaped_text(Stream::Stderr)
            .contains("expected failure")
    );
}
#[test]
fn real_leaf_timeout_is_terminated_and_reaped() {
    let outcome = run("wait", 1, 4096, false);
    assert_eq!(outcome.status.signal(), Some(Signal::TERM.as_raw()));
    assert_eq!(outcome.decision, Decision::Terminate(Reason::TimedOut));
    assert!(outcome.term_sent && !outcome.kill_sent);
    assert!(outcome.elapsed >= Duration::from_secs(1));
}
#[test]
fn real_leaf_cancellation_is_terminated_and_reaped() {
    let outcome = run("wait", 10, 4096, true);
    assert_eq!(outcome.status.signal(), Some(Signal::TERM.as_raw()));
    assert_eq!(outcome.decision, Decision::Terminate(Reason::Cancelled));
    assert!(outcome.term_sent && !outcome.kill_sent);
}
#[test]
fn real_leaf_output_flood_is_bounded_and_terminated() {
    let outcome = run("flood", 10, 1024, false);
    assert_eq!(outcome.status.signal(), Some(Signal::TERM.as_raw()));
    assert_eq!(outcome.decision, Decision::Terminate(Reason::OutputLimit));
    assert!(outcome.term_sent && !outcome.kill_sent);
    assert_eq!(outcome.output.retained_bytes(), 1024);
    assert!(outcome.output.truncated());
}
#[test]
fn real_leaf_ignoring_term_is_force_killed_after_grace() {
    let outcome = run("ignore-term", 10, 4096, true);
    assert_eq!(outcome.status.signal(), Some(Signal::KILL.as_raw()));
    assert_eq!(outcome.decision, Decision::ForceKill(Reason::Cancelled));
    assert!(outcome.term_sent && outcome.kill_sent);
    assert!(outcome.elapsed >= TERMINATION_GRACE);
}
