use super::*;

fn plan() -> BuildPlan {
    let (catalog, input) = crate::tests::fixture();
    BuildPlan::validate(input, &catalog).unwrap()
}

fn artifact() -> ArtifactMetadata {
    // Synthetic claims only; no artifact is built, read or retained by these tests.
    ArtifactMetadata::new(Sha256::from_hex(&"ab".repeat(32)).unwrap(), 123).unwrap()
}

fn job(plan: &BuildPlan) -> BuildJob {
    BuildJob::queued(JobId::from_bytes([1; 16]), plan.clone())
}

fn event(job: &BuildJob, state: State) -> RunnerEvent {
    RunnerEvent {
        job: job.id(),
        plan: job.plan().clone(),
        state,
    }
}

fn transition(job: &mut BuildJob, state: State) -> Result<(), Error> {
    job.apply(event(job, state))
}

#[test]
fn queued_attempt_owns_immutable_plan_and_has_no_artifact() {
    let mut input_copy = plan();
    let job = job(&input_copy);
    input_copy.0.binary = "edited-after-queue".into();
    assert_eq!(job.id().to_bytes(), [1; 16]);
    assert_eq!(job.state(), &State::Queued);
    assert_eq!(job.plan().details().binary, "ExampleService");
}

#[test]
fn success_requires_start_then_plan_bound_event_with_artifact() {
    let plan = plan();
    let mut job = job(&plan);
    let metadata = artifact();
    assert_eq!(
        transition(&mut job, State::Succeeded(metadata.clone())),
        Err(Error::InvalidTransition)
    );
    assert_eq!(job.state(), &State::Queued);
    transition(&mut job, State::Running).unwrap();
    transition(&mut job, State::Succeeded(metadata.clone())).unwrap();
    assert_eq!(job.state(), &State::Succeeded(metadata));
    assert!(job.plan() == &plan);
}

#[test]
fn transition_matrix_preserves_records_on_every_rejected_edge() {
    let plan = plan();
    let states = [
        State::Queued,
        State::Running,
        State::Succeeded(artifact()),
        State::Failed(Failure::Preparation),
        State::Failed(Failure::ExitFailure),
        State::Failed(Failure::TimedOut),
        State::Failed(Failure::Terminated),
        State::Failed(Failure::ArtifactRejected),
        State::Cancelled,
    ];
    // Indices: queued may start, fail preparation or cancel; running may finish.
    let allowed = [
        (0, 1),
        (0, 3),
        (0, 8),
        (1, 2),
        (1, 3),
        (1, 4),
        (1, 5),
        (1, 6),
        (1, 7),
        (1, 8),
    ];
    for (from, before) in states.iter().enumerate() {
        for (to, next) in states.iter().enumerate() {
            let mut job = job(&plan);
            match before {
                State::Queued => {}
                State::Running | State::Cancelled | State::Failed(Failure::Preparation) => {
                    transition(&mut job, before.clone()).unwrap();
                }
                _ => {
                    transition(&mut job, State::Running).unwrap();
                    transition(&mut job, before.clone()).unwrap();
                }
            }
            let result = transition(&mut job, next.clone());
            if allowed.contains(&(from, to)) {
                assert_eq!(result, Ok(()), "{before:?} -> {next:?}");
                assert_eq!(job.state(), next);
            } else {
                assert_eq!(
                    result,
                    Err(Error::InvalidTransition),
                    "{before:?} -> {next:?}"
                );
                assert_eq!(job.state(), before);
            }
            assert!(job.plan() == &plan);
            assert_eq!(job.id().to_bytes(), [1; 16]);
        }
    }
}

#[test]
fn a_result_from_another_attempt_cannot_complete_the_same_plan() {
    let plan = plan();
    let mut job = job(&plan);
    transition(&mut job, State::Running).unwrap();
    let other = BuildJob::queued(JobId::from_bytes([2; 16]), plan);
    assert_eq!(
        job.apply(event(&other, State::Succeeded(artifact()))),
        Err(Error::WrongJob)
    );
    assert_eq!(job.state(), &State::Running);
}

#[test]
fn changed_plan_cannot_start_or_complete_an_attempt() {
    let (catalog, input) = crate::tests::fixture();
    let original = BuildPlan::validate(input.clone(), &catalog).unwrap();
    // Full-field equality already has dedicated BuildPlan tests; exercise its
    // use at this boundary with source, environment, dependency and limit changes.
    let changes: &[fn(&mut crate::PlanInput)] = &[
        crate::tests::change_source_revision,
        |p| p.environment[0].value = "other".into(),
        |p| p.limits.elapsed_seconds += 1,
        |p| p.rust_version = "1.98.0".into(),
        crate::tests::change_source_bytes,
        |p| {
            p.dependencies = devoption_registry::CargoMaterialization::new(
                p.dependencies.request().clone(),
                Sha256::from_hex(&"cd".repeat(32)).unwrap(),
                200,
            );
        },
    ];
    for change in changes {
        let mut changed = input.clone();
        change(&mut changed);
        let different = BuildPlan::validate(changed, &catalog).unwrap();
        for next in [State::Running, State::Succeeded(artifact())] {
            let mut job = job(&original);
            if matches!(next, State::Succeeded(_)) {
                transition(&mut job, State::Running).unwrap();
            }
            let before = job.state().clone();
            let event = RunnerEvent {
                job: job.id(),
                plan: different.clone(),
                state: next,
            };
            assert_eq!(job.apply(event), Err(Error::WrongPlan));
            assert_eq!(job.state(), &before);
            assert!(job.plan() == &original);
        }
    }
}

#[test]
fn queued_cancellation_blocks_late_start_and_running_cancellation_needs_runner() {
    let plan = plan();
    let mut queued = job(&plan);
    let late_start = event(&queued, State::Running);
    queued.cancel_queued().unwrap();
    assert_eq!(queued.apply(late_start), Err(Error::InvalidTransition));
    assert_eq!(queued.cancel_queued(), Err(Error::InvalidTransition));
    assert_eq!(queued.state(), &State::Cancelled);

    let mut running = job(&plan);
    transition(&mut running, State::Running).unwrap();
    assert_eq!(running.cancel_queued(), Err(Error::InvalidTransition));
    assert_eq!(running.state(), &State::Running);
    transition(&mut running, State::Cancelled).unwrap();
    assert_eq!(
        transition(&mut running, State::Succeeded(artifact())),
        Err(Error::InvalidTransition)
    );
    assert_eq!(running.state(), &State::Cancelled);
}

#[test]
fn duplicate_terminal_event_cannot_replace_artifact_or_failure() {
    let plan = plan();
    let mut job = job(&plan);
    transition(&mut job, State::Running).unwrap();
    let duplicate = event(&job, State::Succeeded(artifact()));
    transition(&mut job, State::Succeeded(artifact())).unwrap();
    assert_eq!(job.apply(duplicate), Err(Error::InvalidTransition));
    assert_eq!(job.cancel_queued(), Err(Error::InvalidTransition));
    assert_eq!(job.state(), &State::Succeeded(artifact()));
}

#[test]
fn artifact_metadata_rejects_empty_bytes_without_claiming_verification() {
    let digest = Sha256::from_hex(&"ab".repeat(32)).unwrap();
    assert_eq!(ArtifactMetadata::new(digest, 0), Err(Error::EmptyArtifact));
    let metadata = ArtifactMetadata::new(digest, u64::MAX).unwrap();
    assert!(metadata.sha256() == &digest);
    assert_eq!(metadata.bytes(), u64::MAX);
    // Byte limits and content verification belong to the future runner, not this
    // descriptor. Merely constructing even valid metadata does not change a job.
    let job = job(&plan());
    let _claim = State::Succeeded(metadata);
    assert_eq!(job.state(), &State::Queued);
}
