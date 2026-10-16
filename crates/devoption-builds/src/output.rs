//! Bounded in-memory output supplied by a caller, not evidence of execution.
//! No process readers, persistence, secret redaction or job-state transitions.

use crate::BuildPlan;
use std::{fmt, fmt::Write};

pub const MAX_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// The future runner must stop the complete job when `limit_reached` is true.
/// This notification does not terminate anything or establish a successful exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Append {
    pub retained: usize,
    pub limit_reached: bool,
}

/// Private bytes with one combined budget and no Debug or raw-byte accessor.
/// Streams retain their own order; no cross-stream chronological order is claimed.
pub struct BuildOutput {
    limit: usize,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
}

impl BuildOutput {
    pub fn for_plan(plan: &BuildPlan) -> Result<Self, InvalidLimit> {
        Self::new(plan.details().limits.log_bytes as usize)
    }

    fn new(limit: usize) -> Result<Self, InvalidLimit> {
        if !(1..=MAX_BYTES).contains(&limit) {
            return Err(InvalidLimit);
        }
        Ok(Self {
            limit,
            // Two fixed-capacity buffers: at most 2 MiB reserved, with at most
            // 1 MiB total initialized/retained bytes across both streams.
            stdout: Vec::with_capacity(limit),
            stderr: Vec::with_capacity(limit),
            truncated: false,
        })
    }

    /// Retain only the available prefix. Work/copying is bounded by remaining
    /// capacity even for a larger input; rejected suffixes are never decoded.
    pub fn append(&mut self, stream: Stream, bytes: &[u8]) -> Append {
        let retained = bytes.len().min(self.limit - self.retained_bytes());
        self.truncated |= retained != bytes.len();
        let destination = match stream {
            Stream::Stdout => &mut self.stdout,
            Stream::Stderr => &mut self.stderr,
        };
        destination.extend_from_slice(&bytes[..retained]);
        Append {
            retained,
            limit_reached: self.limit_reached(),
        }
    }

    pub fn retained_bytes(&self) -> usize {
        self.stdout.len() + self.stderr.len()
    }

    pub fn limit_reached(&self) -> bool {
        self.retained_bytes() == self.limit
    }

    /// Sticky evidence that at least one supplied byte was discarded. An exact
    /// fill also requires stopping, even when no discarded byte was observed.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Escaped HTML text for a preformatted text node, never markup, attributes,
    /// JavaScript or a terminal. Rendering does not consume or finish a stream.
    /// Decode after collection so chunk boundaries cannot corrupt valid UTF-8.
    /// Invalid/incomplete sequences become U+FFFD; an incomplete live suffix may
    /// therefore display differently after a later append completes it.
    pub fn escaped_text(&self, stream: Stream) -> String {
        let bytes = match stream {
            Stream::Stdout => &self.stdout,
            Stream::Stderr => &self.stderr,
        };
        let mut result = String::with_capacity(bytes.len() * 6);
        for character in String::from_utf8_lossy(bytes).chars() {
            match character {
                '&' => result.push_str("&amp;"),
                '<' => result.push_str("&lt;"),
                '>' => result.push_str("&gt;"),
                '"' => result.push_str("&quot;"),
                '\'' => result.push_str("&#39;"),
                '\\' => result.push_str("\\\\"),
                '\n' | '\t' => result.push(character),
                c if c.is_control() || bidi_format(c) => {
                    // Escapes contain only ASCII and are distinguishable from
                    // literal backslashes. No control sequence reaches the sink.
                    write!(result, "\\u{{{:x}}}", c as u32).expect("writing to String");
                }
                c => result.push(c),
            }
        }
        result
    }
}

fn bidi_format(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidLimit;
impl fmt::Display for InvalidLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("build output limit must be between 1 and 1048576 bytes")
    }
}
impl std::error::Error for InvalidLimit {}

#[cfg(test)]
mod tests;
