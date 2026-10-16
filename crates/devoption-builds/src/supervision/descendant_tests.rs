//! Linux-only subreaper evidence in a separate process, never the main test runner.
use super::*;
use rustix::{
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    process::{
        Pid, Signal, WaitId, WaitIdOptions, WaitOptions, WaitStatus, getpid, kill_process_group,
        set_child_subreaper, waitid, waitpgid,
    },
};
use std::{
    fs,
    io::{ErrorKind, Read},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    thread,
};
const HELPER: &str = "supervision::descendant_tests::isolated_subreaper_fixture";

struct GroupGuard {
    id: Pid,
    may_signal: bool,
}
impl GroupGuard {
    fn kill(&mut self) -> rustix::io::Result<()> {
        if self.may_signal {
            // The leader has not been reaped, so its group ID cannot be reused.
            let result = kill_process_group(self.id, Signal::KILL);
            match result {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => return Err(error),
            }
            self.may_signal = false;
        }
        Ok(())
    }
    fn reap(&self) -> rustix::io::Result<Vec<(Pid, WaitStatus)>> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut statuses = Vec::new();
        loop {
            match waitpgid(self.id, WaitOptions::NOHANG) {
                Ok(Some(status)) => {
                    statuses.push(status);
                    if statuses.len() > 2 {
                        return Err(rustix::io::Errno::OVERFLOW);
                    }
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return Err(rustix::io::Errno::TIMEDOUT);
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(rustix::io::Errno::CHILD) => return Ok(statuses),
                Err(error) => return Err(error),
            }
        }
    }
}
impl Drop for GroupGuard {
    fn drop(&mut self) {
        if self.may_signal {
            let _ = self.kill();
        }
        let _ = self.reap();
    }
}
fn read(pipe: &mut impl Read, stream: Stream, monitor: &mut Supervision) -> bool {
    let mut bytes = [0; 4096];
    match pipe.read(&mut bytes) {
        Ok(0) => true,
        Ok(count) => {
            monitor.append(stream, &bytes[..count]);
            false
        }
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
            false
        }
        Err(error) => panic!("fixture pipe failed: {error}"),
    }
}

#[test]
fn orphan_pipe_holder_requires_kill_and_reaping_after_leader_success() {
    // Subreaper state is process-wide. Never enable it inside the shared runner.
    let receipt = tempfile::tempdir().unwrap();
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            HELPER,
            "--ignored",
            "--test-threads=1",
            "--nocapture",
        ])
        .current_dir(receipt.path())
        .env_clear()
        .env("LANG", "C.UTF-8")
        .env("DEVOPTION_SUBREAPER_FIXTURE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if let Some(status) = helper.try_wait().unwrap() {
            assert!(status.success());
            assert_eq!(
                fs::read(receipt.path().join("receipt")).unwrap(),
                b"reaped:2; EOF:2"
            );
            break;
        }
        if Instant::now() >= deadline {
            helper.kill().unwrap();
            helper.wait().unwrap();
            panic!("isolated subreaper exceeded its deadline");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "launched only by the bounded outer test in a separate process"]
// waitpgid below reaps the leader and orphan after signaling; Child::wait
// would release the leader identity too early.
#[allow(clippy::zombie_processes)]
fn isolated_subreaper_fixture() {
    if std::env::var("DEVOPTION_SUBREAPER_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    set_child_subreaper(Some(getpid())).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let sentinel = temporary.path().join("unrelated-sentinel");
    fs::write(&sentinel, b"preserve me").unwrap();
    let mut plan = crate::review_context::tests::plan();
    plan.0.limits.elapsed_seconds = 10;
    plan.0.limits.log_bytes = 4096;
    let mut monitor = Supervision::for_plan(&plan).unwrap();
    let mut leader = Command::new("/usr/bin/python3")
        .args([
            "-I",
            "-B",
            "-S",
            "-u",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/supervision/orphan_fixture.py"
            ),
        ])
        .env_clear()
        .env("LANG", "C.UTF-8")
        .current_dir(temporary.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = Pid::from_raw(leader.id() as i32).unwrap();
    assert!(pid.as_raw_nonzero().get() > 1);
    let mut guard = GroupGuard {
        id: pid,
        may_signal: true,
    };
    let mut stdout = leader.stdout.take().unwrap();
    let mut stderr = leader.stderr.take().unwrap();
    fcntl_setfl(&stdout, fcntl_getfl(&stdout).unwrap() | OFlags::NONBLOCK).unwrap();
    fcntl_setfl(&stderr, fcntl_getfl(&stderr).unwrap() | OFlags::NONBLOCK).unwrap();
    let start = Instant::now();
    let mut term_sent = false;
    let (mut out_eof, mut err_eof) = (false, false);
    loop {
        assert!(start.elapsed() < Duration::from_secs(15));
        if !out_eof {
            out_eof = read(&mut stdout, Stream::Stdout, &mut monitor);
        }
        if !err_eof {
            err_eof = read(&mut stderr, Stream::Stderr, &mut monitor);
        }
        if !term_sent
            && let Some(status) = waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )
            .unwrap()
        {
            assert_eq!(status.exit_status(), Some(0));
            // The exited leader is deliberately not reaped yet. Its child
            // still holds both pipes; leader success cannot complete a job.
            assert!(!out_eof && !err_eof);
            assert_eq!(monitor.cancel(), Decision::Terminate(Reason::Cancelled));
            kill_process_group(pid, Signal::TERM).unwrap();
            term_sent = true;
        }
        if monitor.poll() == Decision::ForceKill(Reason::Cancelled) {
            assert!(term_sent);
            guard.kill().unwrap();
            let statuses = guard.reap().unwrap();
            assert_eq!(statuses.len(), 2);
            let leader_status = statuses.iter().find(|(id, _)| *id == pid).unwrap().1;
            let descendant_status = statuses.iter().find(|(id, _)| *id != pid).unwrap().1;
            assert_eq!(leader_status.exit_status(), Some(0));
            assert_eq!(
                descendant_status.terminating_signal(),
                Some(Signal::KILL.as_raw())
            );
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    // All owned fixture processes have been reaped; drain the bounded remainder.
    let deadline = Instant::now() + Duration::from_secs(1);
    while !out_eof || !err_eof {
        assert!(Instant::now() < deadline, "descendant pipes stayed open");
        if !out_eof {
            out_eof = read(&mut stdout, Stream::Stdout, &mut monitor);
        }
        if !err_eof {
            err_eof = read(&mut stderr, Stream::Stderr, &mut monitor);
        }
    }
    assert!(
        monitor
            .output()
            .escaped_text(Stream::Stdout)
            .contains("CHILD_READY")
    );
    assert!(
        monitor
            .output()
            .escaped_text(Stream::Stderr)
            .contains("CHILD_STDERR")
    );
    assert_eq!(fs::read(&sentinel).unwrap(), b"preserve me");
    // The outer test requires this, so a mismatched test filter cannot pass by
    // running zero tests. The helper cwd is its separate disposable receipt tree.
    fs::write("receipt", b"reaped:2; EOF:2").unwrap();
}
