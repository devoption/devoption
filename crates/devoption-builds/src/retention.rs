//! Pure retention planning over a caller's complete, consistent snapshot.
//! No storage, deletion, execution authority or disk-reclamation claim.

use crate::jobs::{JobId, State};
use devoption_projects::ProjectId;
use std::{collections::HashSet, fmt};

/// Maximum retained terminal attempts selected by one project's policy.
/// Protected exceptions can exceed this bound; it is not a lifetime attempt cap.
pub const MAX_RETAINED: u16 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy(u16);

impl Default for Policy {
    fn default() -> Self {
        Self(10)
    }
}

impl Policy {
    pub fn new(count: u16) -> Result<Self, Error> {
        if !(1..=MAX_RETAINED).contains(&count) {
            return Err(Error::InvalidLimit);
        }
        Ok(Self(count))
    }

    pub fn count(self) -> u16 {
        self.0
    }
}

/// Caller-supplied observations, not verified lifecycle or ownership evidence.
pub struct Attempt<'a> {
    pub id: JobId,
    pub project: ProjectId,
    pub state: &'a State,
    /// Durable, nonzero, unique per-project terminal transition sequence.
    /// Admission order, wall-clock time and input position are not substitutes.
    pub completion: Option<u64>,
    /// Explicit process reconciliation, especially after lost supervision.
    pub cleanup_confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepReason {
    InFlight,
    CleanupUnconfirmed,
    Referenced,
    Newest,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// Eligible IDs, oldest completion first. These are not deletion permits.
    pub candidates: Vec<JobId>,
    /// Retained IDs and the primary reason, in snapshot order.
    pub retained: Vec<(JobId, KeepReason)>,
}

/// Count all terminal outcomes toward the newest-N window. Older protected
/// attempts are retained in addition to that window, rather than consuming it.
/// `references` must include every saved release (including rollback evidence)
/// and active promotion. Unknown references and ambiguous input fail closed.
///
/// The caller must supply a complete snapshot across projects while serialized
/// with admission/promotion. Replan and recheck references/owned paths under that
/// same serialization immediately before any future removal operation.
pub fn plan(
    project: ProjectId,
    policy: Policy,
    attempts: &[Attempt<'_>],
    references: &[JobId],
) -> Result<Plan, Error> {
    let mut ids = HashSet::new();
    let mut completions = HashSet::new();
    for attempt in attempts {
        if !ids.insert(attempt.id) {
            return Err(Error::InvalidSnapshot);
        }
        match (in_flight(attempt.state), attempt.completion) {
            (true, None) => {}
            (false, Some(sequence))
                if sequence != 0 && completions.insert((attempt.project, sequence)) => {}
            _ => return Err(Error::InvalidSnapshot),
        }
    }
    let references: HashSet<_> = references.iter().copied().collect();
    if references.iter().any(|id| !ids.contains(id)) {
        return Err(Error::UnknownReference);
    }
    let mut terminal: Vec<_> = attempts
        .iter()
        .filter(|attempt| attempt.project == project && !in_flight(attempt.state))
        .collect();
    terminal.sort_unstable_by_key(|attempt| attempt.completion);
    let newest: HashSet<_> = terminal
        .iter()
        .rev()
        .take(usize::from(policy.count()))
        .map(|attempt| attempt.id)
        .collect();
    let mut retained = Vec::new();
    let mut candidates = HashSet::new();
    for attempt in attempts.iter().filter(|attempt| attempt.project == project) {
        let reason = if in_flight(attempt.state) {
            Some(KeepReason::InFlight)
        } else if !attempt.cleanup_confirmed {
            Some(KeepReason::CleanupUnconfirmed)
        } else if references.contains(&attempt.id) {
            Some(KeepReason::Referenced)
        } else if newest.contains(&attempt.id) {
            Some(KeepReason::Newest)
        } else {
            None
        };
        if let Some(reason) = reason {
            retained.push((attempt.id, reason));
        } else {
            candidates.insert(attempt.id);
        }
    }
    Ok(Plan {
        candidates: terminal
            .into_iter()
            .filter(|attempt| candidates.contains(&attempt.id))
            .map(|attempt| attempt.id)
            .collect(),
        retained,
    })
}

fn in_flight(state: &State) -> bool {
    matches!(state, State::Queued | State::Running)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidLimit,
    InvalidSnapshot,
    UnknownReference,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidLimit => "retention count must be between 1 and 1000",
            Self::InvalidSnapshot => {
                "retention snapshot has ambiguous identity or completion order"
            }
            Self::UnknownReference => "retention reference is absent from the complete snapshot",
        })
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
