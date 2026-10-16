use super::*;
use crate::jobs::{ArtifactMetadata, Failure};
use devoption_registry::Sha256;
const MAGIC: &[u8; 9] = b"DVHISTRY\x01";
const WIDTH: usize = 161;
pub(super) const MAX_BYTES: usize = 10 + MAX_JOBS * WIDTH + 32;
fn state_tag(state: &State) -> u8 {
    match state {
        State::Queued => 0,
        State::Running => 1,
        State::Succeeded(_) => 2,
        State::Failed(Failure::Preparation) => 3,
        State::Failed(Failure::ExitFailure) => 4,
        State::Failed(Failure::TimedOut) => 5,
        State::Failed(Failure::Terminated) => 6,
        State::Failed(Failure::ArtifactRejected) => 7,
        State::Cancelled => 8,
        State::Interrupted => 9,
    }
}
pub(super) fn encode(records: &[Record]) -> Result<Vec<u8>, Error> {
    if records.len() > MAX_JOBS {
        return Err(Error::Capacity);
    }
    let mut bytes = MAGIC.to_vec();
    bytes.push(records.len() as u8);
    for r in records {
        bytes.extend(r.id.to_bytes());
        bytes.extend(r.project);
        bytes.extend(r.repository);
        bytes.extend(r.commit.as_bytes());
        bytes.extend(r.source);
        bytes.push(state_tag(&r.state));
        if let State::Succeeded(a) = &r.state {
            bytes.extend(a.sha256().bytes());
            bytes.extend(a.bytes().to_be_bytes());
        } else {
            bytes.extend([0; 40]);
        }
    }
    bytes.extend(*Sha256::digest(&bytes).bytes());
    decode(&bytes)?;
    Ok(bytes)
}
pub(super) fn decode(bytes: &[u8]) -> Result<Vec<Record>, Error> {
    if bytes.len() < 42 || bytes.len() > MAX_BYTES || &bytes[..9] != MAGIC {
        return Err(Error::Corrupt);
    }
    let count = bytes[9] as usize;
    if count > MAX_JOBS || bytes.len() != 10 + count * WIDTH + 32 {
        return Err(Error::Corrupt);
    }
    let end = bytes.len() - 32;
    if Sha256::digest(&bytes[..end]).bytes() != &bytes[end..] {
        return Err(Error::Corrupt);
    }
    let mut records = Vec::new();
    for r in bytes[10..end].as_chunks::<WIDTH>().0 {
        let id = JobId::from_bytes(r[..16].try_into().unwrap());
        if records.iter().any(|old: &Record| old.id == id) {
            return Err(Error::Corrupt);
        }
        let commit = std::str::from_utf8(&r[48..88]).map_err(|_| Error::Corrupt)?;
        if !commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || commit.bytes().all(|b| b == b'0')
        {
            return Err(Error::Corrupt);
        }
        let state = match r[120] {
            0 => State::Queued,
            1 => State::Running,
            2 => {
                let size = u64::from_be_bytes(r[153..161].try_into().unwrap());
                if !(1..=4 * 1024 * 1024).contains(&size) {
                    return Err(Error::Corrupt);
                }
                let hex: String = r[121..153].iter().map(|b| format!("{b:02x}")).collect();
                State::Succeeded(
                    ArtifactMetadata::new(
                        Sha256::from_hex(&hex).map_err(|_| Error::Corrupt)?,
                        size,
                    )
                    .map_err(|_| Error::Corrupt)?,
                )
            }
            3 => State::Failed(Failure::Preparation),
            4 => State::Failed(Failure::ExitFailure),
            5 => State::Failed(Failure::TimedOut),
            6 => State::Failed(Failure::Terminated),
            7 => State::Failed(Failure::ArtifactRejected),
            8 => State::Cancelled,
            9 => State::Interrupted,
            _ => return Err(Error::Corrupt),
        };
        if r[120] != 2 && r[121..].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt);
        }
        records.push(Record {
            id,
            project: r[16..32].try_into().unwrap(),
            repository: r[32..48].try_into().unwrap(),
            commit: commit.into(),
            source: r[88..120].try_into().unwrap(),
            state,
        });
    }
    Ok(records)
}
