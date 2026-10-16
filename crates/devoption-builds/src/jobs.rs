//! In-memory lifecycle records, with no runner, persistence or execution authority.
//! Runner-driven transitions require an opaque event with no production issuer yet.

use crate::BuildPlan;
use devoption_registry::Sha256;
use std::{fmt, num::NonZeroU64};

/// Identity of one attempt, independent of the plan. The history store allocates
/// fresh persisted IDs; this value type alone does not ensure uniqueness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JobId([u8; 16]);

impl JobId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn to_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// Claimed metadata for the single binary named by the job's plan. Construction
/// does not read or verify bytes, select a path, grant approval or prove success.
#[derive(Clone, PartialEq, Eq)]
pub struct ArtifactMetadata {
    sha256: Sha256,
    bytes: NonZeroU64,
}

impl fmt::Debug for ArtifactMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArtifactMetadata")
            .field("sha256", &self.sha256.bytes())
            .field("bytes", &self.bytes)
            .finish()
    }
}

impl ArtifactMetadata {
    pub fn new(sha256: Sha256, bytes: u64) -> Result<Self, Error> {
        Ok(Self {
            sha256,
            bytes: NonZeroU64::new(bytes).ok_or(Error::EmptyArtifact)?,
        })
    }

    pub fn sha256(&self) -> &Sha256 {
        &self.sha256
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.get()
    }
}

/// Fixed outcome categories, without unbounded logs or caller-supplied paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Preparation,
    ExitFailure,
    TimedOut,
    Terminated,
    ArtifactRejected,
}

/// A value describing a recorded state, not evidence of execution. Constructing
/// one cannot change a job. Only succeeded states carry artifact metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Running,
    Succeeded(ArtifactMetadata),
    Failed(Failure),
    Cancelled,
    /// Supervisor continuity was lost; process termination is not established.
    Interrupted,
}

/// Sealed handoff from a future trusted runner. There is deliberately no public
/// constructor, deserializer or production issuer. Only module-local tests issue
/// synthetic events. Before adding an issuer, it must establish approval, the
/// exact attempt/plan, actual process state and cleanup; success additionally needs
/// a successful exit and verification of the actual plan-selected artifact bytes.
/// This software boundary is not OS isolation or cryptographic authentication.
///
/// Callers cannot wrap a status assignment as a trusted result:
/// ```compile_fail
/// use devoption_builds::{BuildPlan, jobs::{JobId, RunnerEvent, State}};
/// fn fabricate(job: JobId, plan: BuildPlan, state: State) -> RunnerEvent {
///     RunnerEvent { job, plan, state }
/// }
/// ```
pub struct RunnerEvent {
    job: JobId,
    plan: BuildPlan,
    state: State,
}

/// One immutable plan and its constrained recorded lifecycle. Neither creating a
/// queued record nor applying an event is an execution or approval operation.
/// Plan identity here is exact BuildPlan value equality, not a serialized digest.
///
/// ```compile_fail
/// use devoption_builds::jobs::{BuildJob, State};
/// fn fabricate(job: &mut BuildJob) {
///     job.state = State::Running;
/// }
/// ```
pub struct BuildJob {
    id: JobId,
    plan: BuildPlan,
    state: State,
}

impl BuildJob {
    pub fn queued(id: JobId, plan: BuildPlan) -> Self {
        Self {
            id,
            plan,
            state: State::Queued,
        }
    }

    /// Reconstruct an unfinished historical attempt only as interrupted. This
    /// never restores approval, live runner state, success or artifact authority.
    /// A future store must validate the saved identity/plan and reconcile any
    /// surviving children before admitting new work.
    pub fn recover_interrupted(id: JobId, plan: BuildPlan) -> Self {
        Self {
            id,
            plan,
            state: State::Interrupted,
        }
    }

    /// Record lost supervisor continuity without claiming cancellation or cleanup.
    /// The caller must separately revoke pending approval and reconcile children.
    /// Terminal records remain immutable; retries require a fresh attempt.
    pub fn interrupt(&mut self) -> Result<(), Error> {
        if !matches!(self.state, State::Queued | State::Running) {
            return Err(Error::InvalidTransition);
        }
        self.state = State::Interrupted;
        Ok(())
    }

    pub fn id(&self) -> JobId {
        self.id
    }

    pub fn plan(&self) -> &BuildPlan {
        &self.plan
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Cancel an attempt that has not started. A running job requires a runner's
    /// confirmed cancellation/cleanup event; a request alone cannot mark it stopped.
    pub fn cancel_queued(&mut self) -> Result<(), Error> {
        if self.state != State::Queued {
            return Err(Error::InvalidTransition);
        }
        self.state = State::Cancelled;
        Ok(())
    }

    /// Consume a runner event only when its exact attempt and plan match and its
    /// transition is valid. Errors leave the record untouched. Terminal states
    /// cannot be rewritten or retried; retries need a fresh attempt identity.
    pub fn apply(&mut self, event: RunnerEvent) -> Result<(), Error> {
        if event.job != self.id {
            return Err(Error::WrongJob);
        }
        if event.plan != self.plan {
            return Err(Error::WrongPlan);
        }
        let allowed = matches!(
            (&self.state, &event.state),
            (
                State::Queued,
                State::Running | State::Failed(Failure::Preparation) | State::Cancelled
            ) | (
                State::Running,
                State::Succeeded(_) | State::Failed(_) | State::Cancelled
            )
        );
        if !allowed {
            return Err(Error::InvalidTransition);
        }
        self.state = event.state;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    EmptyArtifact,
    WrongJob,
    WrongPlan,
    InvalidTransition,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyArtifact => "artifact metadata must describe nonempty bytes",
            Self::WrongJob => "runner event belongs to a different job attempt",
            Self::WrongPlan => "runner event belongs to a different build plan",
            Self::InvalidTransition => "build job transition is not allowed",
        })
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
