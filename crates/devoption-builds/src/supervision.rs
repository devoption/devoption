//! Monotonic stop decisions only. No processes, timers, signals or cleanup proof.
use crate::{
    BuildPlan,
    output::{BuildOutput, Stream},
};
use std::{
    fmt,
    time::{Duration, Instant},
};

pub const TERMINATION_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Cancelled,
    TimedOut,
    OutputLimit,
}

/// Required work for a future runner, never evidence that the work was done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Continue,
    Terminate(Reason),
    ForceKill(Reason),
}

struct Stop {
    reason: Reason,
    at: Instant,
}

/// One caller-owned observation window with private output and monotonic timing.
/// Constructing it grants no approval or permission to start a process.
pub struct Supervision {
    deadline: Instant,
    stop: Option<Stop>,
    output: BuildOutput,
}
impl Supervision {
    pub fn for_plan(plan: &BuildPlan) -> Result<Self, InvalidLimits> {
        Self::new(plan, Instant::now())
    }
    fn new(plan: &BuildPlan, now: Instant) -> Result<Self, InvalidLimits> {
        let seconds = plan.details().limits.elapsed_seconds;
        if !(1..=60).contains(&seconds) {
            return Err(InvalidLimits);
        }
        let deadline = now
            .checked_add(Duration::from_secs(seconds.into()))
            .ok_or(InvalidLimits)?;
        Ok(Self {
            deadline,
            stop: None,
            output: BuildOutput::for_plan(plan).map_err(|_| InvalidLimits)?,
        })
    }
    /// Polling is required: this object has no background timer or wakeup.
    pub fn poll(&mut self) -> Decision {
        self.decision_at(Instant::now())
    }
    pub fn cancel(&mut self) -> Decision {
        self.cancel_at(Instant::now())
    }
    /// Late output may still be retained within the original combined budget.
    /// Event time is observed at entry; copying does not reset any deadline.
    pub fn append(&mut self, stream: Stream, bytes: &[u8]) -> Decision {
        self.append_at(stream, bytes, Instant::now())
    }
    /// Immutable access to escaped text and cap/truncation metadata only.
    pub fn output(&self) -> &BuildOutput {
        &self.output
    }

    fn cancel_at(&mut self, now: Instant) -> Decision {
        self.decision_at(now);
        self.stop_at(Reason::Cancelled, now);
        self.decision_at(now)
    }
    fn append_at(&mut self, stream: Stream, bytes: &[u8], now: Instant) -> Decision {
        self.decision_at(now);
        if self.output.append(stream, bytes).limit_reached {
            self.stop_at(Reason::OutputLimit, now);
        }
        self.decision_at(now)
    }
    fn stop_at(&mut self, reason: Reason, at: Instant) {
        if self.stop.is_none() {
            self.stop = Some(Stop { reason, at });
        }
    }
    fn decision_at(&mut self, now: Instant) -> Decision {
        if now >= self.deadline {
            self.stop_at(Reason::TimedOut, self.deadline);
        }
        match &self.stop {
            None => Decision::Continue,
            Some(stop) if now.saturating_duration_since(stop.at) >= TERMINATION_GRACE => {
                Decision::ForceKill(stop.reason)
            }
            Some(stop) => Decision::Terminate(stop.reason),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidLimits;
impl fmt::Display for InvalidLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("build supervision requires initial-policy elapsed and output limits")
    }
}
impl std::error::Error for InvalidLimits {}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod process_tests;
