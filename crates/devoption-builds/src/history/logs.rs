use super::*;
use crate::output::{BuildOutput, persistence::MAX_SNAPSHOT};

fn filename(id: JobId) -> String {
    let hex: String = id
        .to_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("build-output-{hex}.bin")
}
impl History {
    /// Persist the last output before a terminal summary makes further output
    /// immutable. If summary publication fails, recovery retains an unfinished
    /// attempt with its last durable log; this is not a multi-file transaction.
    pub fn checkpoint(&mut self, job: &BuildJob, output: &BuildOutput) -> Result<(), Error> {
        self.save_output(job, output)?;
        self.record(job)
    }

    /// Save an append-only snapshot for a known unfinished attempt. Caller-supplied
    /// output does not prove execution. Checkpoint before persisting terminal state.
    pub fn save_output(&mut self, job: &BuildJob, output: &BuildOutput) -> Result<(), Error> {
        let records = self.load()?;
        let record = records
            .iter()
            .find(|record| record.id == job.id())
            .ok_or(Error::UnknownAttempt)?;
        if !record.same_identity(&Record::from_job(job)) || !output.matches_limit(job.plan()) {
            return Err(Error::Changed);
        }
        if !matches!(record.state, State::Queued | State::Running) {
            return Err(Error::Transition);
        }
        let name = filename(job.id());
        if let Some(bytes) = self.read_snapshot(&name, MAX_SNAPSHOT)? {
            let old = BuildOutput::restore(&bytes, job.id())?;
            if !output.extends(&old) {
                return Err(Error::Changed);
            }
        }
        self.publish_snapshot(
            &name,
            &output.snapshot(job.id()),
            MAX_SNAPSHOT,
            |file, bytes| file.write_all(bytes),
        )
    }
    /// None means no saved checkpoint, not an empty or complete log. Historical
    /// output has the same escaped-text interface and grants no execution authority.
    pub fn load_output(&self, id: JobId) -> Result<Option<BuildOutput>, Error> {
        if !self.load()?.iter().any(|record| record.id == id) {
            return Err(Error::UnknownAttempt);
        }
        self.read_snapshot(&filename(id), MAX_SNAPSHOT)?
            .map(|bytes| BuildOutput::restore(&bytes, id))
            .transpose()
    }
}
#[cfg(test)]
mod tests;
