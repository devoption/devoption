use super::*;
use crate::jobs::{ArtifactMetadata, Failure};
use devoption_registry::Sha256;

fn project(value: u8) -> ProjectId {
    ProjectId::from_bytes([value; 16])
}

fn id(value: u64) -> JobId {
    JobId::from_bytes(u128::from(value).to_le_bytes())
}

fn attempt(sequence: u64, state: &State) -> Attempt<'_> {
    Attempt {
        id: id(sequence),
        project: project(1),
        state,
        completion: Some(sequence),
        cleanup_confirmed: true,
    }
}

#[test]
fn limits_are_positive_bounded_and_default_to_ten() {
    assert_eq!(Policy::default().count(), 10);
    for count in [1, 10, MAX_RETAINED] {
        assert_eq!(Policy::new(count).unwrap().count(), count);
    }
    for count in [0, MAX_RETAINED + 1, u16::MAX] {
        assert_eq!(Policy::new(count), Err(Error::InvalidLimit));
    }
}

#[test]
fn completion_order_counts_all_terminal_outcomes_and_ignores_input_order() {
    let states = [
        State::Succeeded(ArtifactMetadata::new(Sha256::digest(b"fixture"), 1).unwrap()),
        State::Failed(Failure::ExitFailure),
        State::Cancelled,
        State::Interrupted,
    ];
    let mut attempts: Vec<_> = states
        .iter()
        .enumerate()
        .map(|(i, state)| attempt(i as u64 + 1, state))
        .collect();
    // Identity and completion order deliberately disagree.
    attempts[0].id = id(100);
    attempts.reverse();
    let result = plan(project(1), Policy::new(2).unwrap(), &attempts, &[]).unwrap();
    assert_eq!(result.candidates, [id(100), id(2)]);
    assert_eq!(
        result.retained,
        [(id(4), KeepReason::Newest), (id(3), KeepReason::Newest)]
    );
}

#[test]
fn protected_exceptions_do_not_displace_newest_window() {
    let mut attempts: Vec<_> = (1..=7).map(|i| attempt(i, &State::Cancelled)).collect();
    attempts[0].state = &State::Queued;
    attempts[0].completion = None;
    attempts[1].state = &State::Running;
    attempts[1].completion = None;
    attempts[2].state = &State::Interrupted;
    attempts[2].cleanup_confirmed = false;
    let result = plan(
        project(1),
        Policy::new(1).unwrap(),
        &attempts,
        &[id(4), id(5)],
    )
    .unwrap();
    assert_eq!(result.candidates, [id(6)]);
    assert_eq!(
        result.retained,
        [
            (id(1), KeepReason::InFlight),
            (id(2), KeepReason::InFlight),
            (id(3), KeepReason::CleanupUnconfirmed),
            (id(4), KeepReason::Referenced),
            (id(5), KeepReason::Referenced),
            (id(7), KeepReason::Newest),
        ]
    );
    // A newly saved release invalidates the earlier candidate on replanning.
    assert!(
        plan(
            project(1),
            Policy::new(1).unwrap(),
            &attempts,
            &[id(4), id(5), id(6)]
        )
        .unwrap()
        .candidates
        .is_empty()
    );
}

#[test]
fn project_policies_are_independent_and_lowering_limit_selects_more() {
    let mut attempts: Vec<_> = (1..=12).map(|i| attempt(i, &State::Cancelled)).collect();
    for i in 1..=12 {
        let mut other = attempt(i, &State::Cancelled);
        other.id = id(i + 100);
        other.project = project(2);
        attempts.push(other);
    }
    assert_eq!(
        plan(project(1), Policy::default(), &attempts, &[])
            .unwrap()
            .candidates,
        [id(1), id(2)]
    );
    assert_eq!(
        plan(project(2), Policy::new(11).unwrap(), &attempts, &[])
            .unwrap()
            .candidates,
        [id(101)]
    );
    assert_eq!(
        plan(project(1), Policy::new(1).unwrap(), &attempts, &[])
            .unwrap()
            .candidates
            .len(),
        11
    );
    assert_eq!(
        plan(project(3), Policy::default(), &attempts, &[]).unwrap(),
        Plan {
            candidates: vec![],
            retained: vec![]
        }
    );
}

#[test]
fn ambiguous_snapshots_and_missing_references_fail_closed() {
    let check = |attempts: &[Attempt<'_>]| {
        assert_eq!(
            plan(project(1), Policy::default(), attempts, &[]),
            Err(Error::InvalidSnapshot)
        );
    };
    check(&[attempt(1, &State::Cancelled), attempt(1, &State::Cancelled)]);
    let mut duplicate_order = attempt(2, &State::Cancelled);
    duplicate_order.completion = Some(1);
    check(&[attempt(1, &State::Cancelled), duplicate_order]);
    let mut missing = attempt(1, &State::Interrupted);
    missing.completion = None;
    check(&[missing]);
    check(&[attempt(0, &State::Cancelled)]);
    check(&[attempt(1, &State::Queued)]);
    check(&[attempt(1, &State::Running)]);
    assert_eq!(
        plan(project(1), Policy::default(), &[], &[id(1)]),
        Err(Error::UnknownReference)
    );
}

#[test]
fn planning_is_repeatable_and_has_no_lifetime_attempt_ceiling() {
    let attempts: Vec<_> = (1..=10_000)
        .map(|i| attempt(i, &State::Cancelled))
        .collect();
    let first = plan(project(1), Policy::default(), &attempts, &[]).unwrap();
    assert_eq!(first.candidates.len(), 9990);
    assert_eq!(first.retained.len(), 10);
    assert_eq!(
        first,
        plan(project(1), Policy::default(), &attempts, &[]).unwrap()
    );
    // Replanning a partially removed snapshot selects the remaining candidates.
    let remaining = &attempts[5000..];
    assert_eq!(
        plan(project(1), Policy::default(), remaining, &[])
            .unwrap()
            .candidates,
        first.candidates[5000..]
    );
}
