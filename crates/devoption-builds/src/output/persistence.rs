//! Internal snapshot codec; persisted bytes are not a public output accessor.
use super::*;
use crate::{history::Error, jobs::JobId};
use devoption_registry::Sha256;
const MAGIC: &[u8; 9] = b"DVOUTPUT\x01";
const HEADER: usize = 38;
pub(crate) const MAX_SNAPSHOT: usize = HEADER + MAX_BYTES + 32;
impl BuildOutput {
    pub(crate) fn matches_limit(&self, plan: &BuildPlan) -> bool {
        self.limit as u32 == plan.details().limits.log_bytes
    }
    pub(crate) fn extends(&self, old: &Self) -> bool {
        self.limit == old.limit
            && self.stdout.starts_with(&old.stdout)
            && self.stderr.starts_with(&old.stderr)
            && (!old.truncated || self.truncated)
    }
    pub(crate) fn snapshot(&self, id: JobId) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(id.to_bytes());
        for value in [self.limit, self.stdout.len(), self.stderr.len()] {
            bytes.extend((value as u32).to_be_bytes());
        }
        bytes.push(u8::from(self.truncated));
        bytes.extend(&self.stdout);
        bytes.extend(&self.stderr);
        bytes.extend(*Sha256::digest(&bytes).bytes());
        bytes
    }
    pub(crate) fn restore(bytes: &[u8], id: JobId) -> Result<Self, Error> {
        if !(HEADER + 32..=MAX_SNAPSHOT).contains(&bytes.len())
            || &bytes[..9] != MAGIC
            || bytes[9..25] != id.to_bytes()
        {
            return Err(Error::Corrupt);
        }
        let value =
            |start| u32::from_be_bytes(bytes[start..start + 4].try_into().unwrap()) as usize;
        let (limit, stdout, stderr) = (value(25), value(29), value(33));
        if !(1..=MAX_BYTES).contains(&limit)
            || stdout > limit
            || stderr > limit - stdout
            || bytes.len() != HEADER + stdout + stderr + 32
            || bytes[37] > 1
            || (bytes[37] == 1 && stdout + stderr != limit)
        {
            return Err(Error::Corrupt);
        }
        let end = bytes.len() - 32;
        if Sha256::digest(&bytes[..end]).bytes() != &bytes[end..] {
            return Err(Error::Corrupt);
        }
        let mut result = Self::new(limit).map_err(|_| Error::Corrupt)?;
        result.stdout.extend(&bytes[HEADER..HEADER + stdout]);
        result.stderr.extend(&bytes[HEADER + stdout..end]);
        result.truncated = bytes[37] == 1;
        Ok(result)
    }
}
