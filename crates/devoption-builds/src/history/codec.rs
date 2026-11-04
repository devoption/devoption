use super::*;
use crate::jobs::{ArtifactMetadata, Failure};
use devoption_registry::Sha256;
const MAGIC: &[u8; 9] = b"DVHISTRY\x02";
const LEGACY_WIDTH: usize = 161;
const WIDTH: usize = 170;
const HEADER: usize = 21;
pub(super) const MAX_BYTES: usize = HEADER + MAX_JOBS * WIDTH + 32;
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
#[cfg(test)]
pub(super) fn encode(records: &[Record]) -> Result<Vec<u8>, Error> {
    encode_with_watermark(records, 0)
}
pub(super) fn encode_with_watermark(records: &[Record], watermark: u64) -> Result<Vec<u8>, Error> {
    if records.len() > MAX_JOBS {
        return Err(Error::Capacity);
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend((records.len() as u32).to_be_bytes());
    let watermark = records
        .iter()
        .filter_map(|r| r.completion)
        .max()
        .unwrap_or(0)
        .max(watermark);
    bytes.extend(watermark.to_be_bytes());
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
        bytes.extend(r.completion.unwrap_or(0).to_be_bytes());
        bytes.push(u8::from(r.cleanup_confirmed));
    }
    bytes.extend(*Sha256::digest(&bytes).bytes());
    decode(&bytes)?;
    Ok(bytes)
}
pub(super) fn decode(bytes: &[u8]) -> Result<Vec<Record>, Error> {
    if bytes.len() < 42
        || bytes.len() > MAX_BYTES
        || &bytes[..8] != b"DVHISTRY"
        || !matches!(bytes[8], 1 | 2)
    {
        return Err(Error::Corrupt);
    }
    let legacy = bytes[8] == 1;
    let (count, start, width) = if legacy {
        (bytes[9] as usize, 10, LEGACY_WIDTH)
    } else {
        if bytes.len() < HEADER + 32 {
            return Err(Error::Corrupt);
        }
        (
            u32::from_be_bytes(bytes[9..13].try_into().unwrap()) as usize,
            HEADER,
            WIDTH,
        )
    };
    if count > MAX_JOBS || bytes.len() != start + count * width + 32 {
        return Err(Error::Corrupt);
    }
    let end = bytes.len() - 32;
    if Sha256::digest(&bytes[..end]).bytes() != &bytes[end..] {
        return Err(Error::Corrupt);
    }
    let mut records = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut sequences = std::collections::HashSet::new();
    for r in bytes[start..end].chunks_exact(width) {
        let id = JobId::from_bytes(r[..16].try_into().unwrap());
        if !seen.insert(id) {
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
        if r[120] != 2 && r[121..161].iter().any(|b| *b != 0) {
            return Err(Error::Corrupt);
        }
        let completion = if legacy {
            // Legacy stores have no completion clock. Establish a deterministic
            // migration order from their existing rows, preserving all evidence.
            terminal(&state).then_some(records.len() as u64 + 1)
        } else {
            match u64::from_be_bytes(r[161..169].try_into().unwrap()) {
                0 => None,
                value => Some(value),
            }
        };
        let cleanup_confirmed = if legacy {
            !matches!(state, State::Interrupted | State::Queued | State::Running)
        } else {
            match r[169] {
                0 => false,
                1 => true,
                _ => return Err(Error::Corrupt),
            }
        };
        if terminal(&state) != completion.is_some()
            || completion.is_some_and(|n| !sequences.insert(n))
            || (!terminal(&state) && cleanup_confirmed)
        {
            return Err(Error::Corrupt);
        }
        records.push(Record {
            id,
            project: r[16..32].try_into().unwrap(),
            repository: r[32..48].try_into().unwrap(),
            commit: commit.into(),
            source: r[88..120].try_into().unwrap(),
            state,
            completion,
            cleanup_confirmed,
        });
    }
    if !legacy {
        let high = u64::from_be_bytes(bytes[13..21].try_into().unwrap());
        if records
            .iter()
            .filter_map(|r| r.completion)
            .any(|n| n > high)
        {
            return Err(Error::Corrupt);
        }
    }
    Ok(records)
}

pub(super) fn watermark(bytes: &[u8]) -> Result<u64, Error> {
    let records = decode(bytes)?;
    Ok(if bytes[8] == 1 {
        records
            .iter()
            .filter_map(|r| r.completion)
            .max()
            .unwrap_or(0)
    } else {
        u64::from_be_bytes(bytes[13..21].try_into().unwrap())
    })
}
