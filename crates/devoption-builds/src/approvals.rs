//! Bounded in-memory approval bookkeeping, not execution authority.
//!
//! A trusted adapter must authenticate the explicit owner action and verify the
//! full review context before recording or consuming approval. This module does
//! neither, and has no runner-event issuer, persistence, HTTP or process I/O.

use crate::review_context::ReviewContext;
use crate::{
    BuildPlan,
    jobs::{BuildJob, JobId, State as JobState},
};
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

pub const LIFETIME: Duration = Duration::from_secs(300);
/// Lifetime attempt bound, including tombstones. Never evict to permit ID reuse.
pub const MAX_ATTEMPTS: usize = 64;

/// Reference to the server-assigned owner, not authentication or a credential.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OwnerId([u8; 16]);
impl OwnerId {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

/// Opaque process-local reference. Clones refer to the same single-use record.
/// It cannot be transferred to a new registry or serialized into authority.
/// ```compile_fail
/// use devoption_builds::approvals::Approval;
/// fn forge() -> Approval { Approval {} }
/// ```
#[derive(Clone)]
pub struct Approval {
    registry: Weak<Mutex<Records>>,
    job: JobId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Consumed,
    Revoked,
    Expired,
    Invalidated,
}

struct Review {
    plan: BuildPlan,
    context: ReviewContext,
}
struct Entry {
    owner: OwnerId,
    issued: Instant,
    status: Status,
    review: Option<Review>,
}
impl Entry {
    fn finish(&mut self, status: Status) {
        self.status = status;
        self.review = None;
    }
    fn expire(&mut self, now: Instant) {
        if self.status == Status::Pending
            && now
                .checked_duration_since(self.issued)
                .is_none_or(|age| age >= LIFETIME)
        {
            self.finish(Status::Expired);
        }
    }
}
#[derive(Default)]
struct Records {
    entries: HashMap<JobId, Entry>,
}

/// Clones share a lock, expiry and tombstones. Default creates a new registry.
/// Keep one registry for the service lifetime; replacement invalidates old handles,
/// but cannot enforce globally unique JobIds across restarts (a future store must).
#[derive(Clone, Default)]
pub struct Approvals {
    records: Arc<Mutex<Records>>,
}

impl Approvals {
    /// Record an explicit review action supplied by a trusted adapter.
    /// `context` is structurally checked review metadata, not proof of verification.
    /// The trusted caller must freshly verify its claims; see docs/build-approvals.md.
    /// No clock, lifetime override, approval flag or execution callback is accepted.
    pub fn approve(
        &self,
        owner: OwnerId,
        job: &BuildJob,
        context: ReviewContext,
    ) -> Result<Approval, Error> {
        self.approve_with_clock(owner, job, context, Instant::now)
    }

    fn approve_with_clock(
        &self,
        owner: OwnerId,
        job: &BuildJob,
        context: ReviewContext,
        clock: impl FnOnce() -> Instant,
    ) -> Result<Approval, Error> {
        let mut records = self.records.lock().map_err(|_| Error::Unavailable)?;
        let now = clock();
        for entry in records.entries.values_mut() {
            entry.expire(now);
        }
        if records.entries.contains_key(&job.id()) {
            return Err(Error::AlreadyReviewed);
        }
        if records.entries.len() >= MAX_ATTEMPTS {
            return Err(Error::Capacity);
        }
        if job.state() != &JobState::Queued {
            return Err(Error::NotQueued);
        }
        if !context.matches_plan(job.plan()) {
            return Err(Error::ReviewChanged);
        }
        records.entries.insert(
            job.id(),
            Entry {
                owner,
                issued: now,
                status: Status::Pending,
                review: Some(Review {
                    plan: job.plan().clone(),
                    context,
                }),
            },
        );
        Ok(Approval {
            registry: Arc::downgrade(&self.records),
            job: job.id(),
        })
    }

    /// Atomically consume once after the caller has freshly verified external state.
    /// This only changes bookkeeping, never the BuildJob or any process. A future
    /// admission adapter must serialize cancellation and verification with admission.
    /// Changed plan/context or nonqueued state permanently invalidates this record.
    /// Wrong owner/attempt cannot invalidate somebody else's approval.
    pub fn consume(
        &self,
        approval: &Approval,
        owner: OwnerId,
        job: &BuildJob,
        context: &ReviewContext,
    ) -> Result<(), Error> {
        self.consume_with_clock(approval, owner, job, context, Instant::now)
    }

    fn consume_with_clock(
        &self,
        approval: &Approval,
        owner: OwnerId,
        job: &BuildJob,
        context: &ReviewContext,
        clock: impl FnOnce() -> Instant,
    ) -> Result<(), Error> {
        self.with_entry(approval, owner, clock, |entry| {
            if job.id() != approval.job {
                return Err(Error::WrongJob);
            }
            pending(entry)?;
            if job.state() != &JobState::Queued {
                entry.finish(Status::Invalidated);
                return Err(Error::NotQueued);
            }
            let review = entry.review.as_ref().ok_or(Error::Unavailable)?;
            if &review.plan != job.plan() || &review.context != context {
                entry.finish(Status::Invalidated);
                return Err(Error::ReviewChanged);
            }
            entry.finish(Status::Consumed);
            Ok(())
        })
    }

    /// Revoke pending approval, including after a failed external verification.
    /// Revocation does not cancel an already admitted job or prove child cleanup.
    pub fn revoke(&self, approval: &Approval, owner: OwnerId) -> Result<(), Error> {
        self.with_entry(approval, owner, Instant::now, |entry| {
            pending(entry)?;
            entry.finish(Status::Revoked);
            Ok(())
        })
    }

    pub fn status(&self, approval: &Approval, owner: OwnerId) -> Result<Status, Error> {
        self.with_entry(approval, owner, Instant::now, |entry| Ok(entry.status))
    }

    fn with_entry<T>(
        &self,
        approval: &Approval,
        owner: OwnerId,
        clock: impl FnOnce() -> Instant,
        operation: impl FnOnce(&mut Entry) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let registry = approval.registry.upgrade().ok_or(Error::WrongRegistry)?;
        if !Arc::ptr_eq(&registry, &self.records) {
            return Err(Error::WrongRegistry);
        }
        let mut records = self.records.lock().map_err(|_| Error::Unavailable)?;
        let entry = records
            .entries
            .get_mut(&approval.job)
            .ok_or(Error::WrongRegistry)?;
        if entry.owner != owner {
            return Err(Error::WrongOwner);
        }
        // Sample after acquiring the lock; queueing cannot extend the deadline.
        entry.expire(clock());
        operation(entry)
    }
}

fn pending(entry: &Entry) -> Result<(), Error> {
    if entry.status == Status::Pending {
        Ok(())
    } else {
        Err(Error::Inactive(entry.status))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    WrongRegistry,
    WrongOwner,
    WrongJob,
    NotQueued,
    AlreadyReviewed,
    Capacity,
    ReviewChanged,
    Inactive(Status),
    Unavailable,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WrongRegistry => "approval does not belong to this registry",
            Self::WrongOwner => "approval owner does not match",
            Self::WrongJob => "approval attempt does not match",
            Self::NotQueued => "approval requires a queued attempt",
            Self::AlreadyReviewed => "attempt already has an approval record",
            Self::Capacity => "approval registry capacity reached",
            Self::ReviewChanged => "approval review changed",
            Self::Inactive(_) => "approval is no longer pending",
            Self::Unavailable => "approval registry unavailable",
        })
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
