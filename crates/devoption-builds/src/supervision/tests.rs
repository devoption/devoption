use super::*;
fn fixture(seconds: u32, bytes: u32) -> (Supervision, Instant) {
    let mut plan = crate::review_context::tests::plan();
    plan.0.limits.elapsed_seconds = seconds;
    plan.0.limits.log_bytes = bytes;
    let now = Instant::now();
    (Supervision::new(&plan, now).unwrap(), now)
}
#[test]
fn exact_deadline_and_grace_boundaries() {
    let (mut monitor, now) = fixture(60, 100);
    let deadline = now + Duration::from_secs(60);
    assert_eq!(
        monitor.decision_at(deadline - Duration::from_nanos(1)),
        Decision::Continue
    );
    assert_eq!(
        monitor.decision_at(deadline),
        Decision::Terminate(Reason::TimedOut)
    );
    assert_eq!(
        monitor.decision_at(deadline + TERMINATION_GRACE - Duration::from_nanos(1)),
        Decision::Terminate(Reason::TimedOut)
    );
    assert_eq!(
        monitor.decision_at(deadline + TERMINATION_GRACE),
        Decision::ForceKill(Reason::TimedOut)
    );
}
#[test]
fn late_poll_never_restarts_timeout_or_grace() {
    let (mut monitor, now) = fixture(1, 100);
    let late = now + Duration::from_secs(30);
    assert_eq!(
        monitor.decision_at(late),
        Decision::ForceKill(Reason::TimedOut)
    );
    assert_eq!(
        monitor.cancel_at(late),
        Decision::ForceKill(Reason::TimedOut)
    );
    assert_eq!(
        monitor.append_at(Stream::Stdout, b"late", late),
        Decision::ForceKill(Reason::TimedOut)
    );
    assert_eq!(monitor.output().escaped_text(Stream::Stdout), "late");
}
#[test]
fn cancellation_is_sticky_across_output_and_timeout() {
    let (mut monitor, now) = fixture(2, 3);
    let cancelled = now + Duration::from_secs(1);
    assert_eq!(
        monitor.cancel_at(cancelled),
        Decision::Terminate(Reason::Cancelled)
    );
    assert_eq!(
        monitor.append_at(Stream::Stderr, b"abcdef", cancelled),
        Decision::Terminate(Reason::Cancelled)
    );
    assert!(monitor.output().truncated());
    assert_eq!(
        monitor.cancel_at(now + Duration::from_secs(5)),
        Decision::Terminate(Reason::Cancelled)
    );
    assert_eq!(
        monitor.decision_at(cancelled + TERMINATION_GRACE),
        Decision::ForceKill(Reason::Cancelled)
    );
    assert_eq!(
        monitor.cancel_at(now + Duration::from_secs(100)),
        Decision::ForceKill(Reason::Cancelled)
    );
}
#[test]
fn combined_output_limit_stops_even_on_exact_fill() {
    let (mut monitor, now) = fixture(60, 4);
    assert_eq!(
        monitor.append_at(Stream::Stdout, b"ab", now),
        Decision::Continue
    );
    let full = now + Duration::from_secs(1);
    assert_eq!(
        monitor.append_at(Stream::Stderr, b"cd", full),
        Decision::Terminate(Reason::OutputLimit)
    );
    assert!(!monitor.output().truncated());
    assert_eq!(
        monitor.cancel_at(full),
        Decision::Terminate(Reason::OutputLimit)
    );
    assert_eq!(
        monitor.append_at(Stream::Stdout, b"more", full + TERMINATION_GRACE),
        Decision::ForceKill(Reason::OutputLimit)
    );
    assert_eq!(monitor.output().retained_bytes(), 4);
    assert!(monitor.output().truncated());
    assert_eq!(monitor.output().escaped_text(Stream::Stderr), "cd");
}
#[test]
fn elapsed_deadline_precedes_new_events_and_empty_chunks_do_not_finish() {
    for cancel in [false, true] {
        let (mut monitor, now) = fixture(1, 1);
        assert_eq!(
            monitor.append_at(Stream::Stdout, b"", now),
            Decision::Continue
        );
        let deadline = now + Duration::from_secs(1);
        let result = if cancel {
            monitor.cancel_at(deadline)
        } else {
            monitor.append_at(Stream::Stdout, b"x", deadline)
        };
        assert_eq!(result, Decision::Terminate(Reason::TimedOut));
    }
}
#[test]
fn repeated_polling_does_not_extend_limits() {
    let (mut monitor, now) = fixture(1, 1);
    for second in 0..10 {
        let result = monitor.decision_at(now + Duration::from_secs(second));
        assert_eq!(
            result,
            match second {
                0 => Decision::Continue,
                1..=5 => Decision::Terminate(Reason::TimedOut),
                _ => Decision::ForceKill(Reason::TimedOut),
            }
        );
    }
}
#[test]
fn reject_out_of_policy_limits_without_authority_or_io() {
    let mut plan = crate::review_context::tests::plan();
    for (seconds, bytes) in [(0, 1), (61, 1), (1, 0), (1, 1_048_577)] {
        plan.0.limits.elapsed_seconds = seconds;
        plan.0.limits.log_bytes = bytes;
        assert_eq!(Supervision::for_plan(&plan).err(), Some(InvalidLimits));
    }
    for (seconds, bytes) in [(1, 1), (60, 1_048_576)] {
        plan.0.limits.elapsed_seconds = seconds;
        plan.0.limits.log_bytes = bytes;
        assert!(Supervision::for_plan(&plan).is_ok());
    }
}
