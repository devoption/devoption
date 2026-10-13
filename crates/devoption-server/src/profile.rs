//! Display-only account metadata; never a credential or permission source.
use std::{fmt, io};

mod store;
pub(crate) use store::ProfileStore;

pub(crate) const MAX_NAME_BYTES: usize = 128;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DisplayName(String);

impl DisplayName {
    /// Empty clears the optional name. Preserve valid Unicode without normalization.
    pub(crate) fn new(value: &str) -> Result<Self, Error> {
        if value.len() > MAX_NAME_BYTES
            || value.trim_matches(|character: char| {
                character.is_whitespace() || character == '\u{feff}'
            }) != value
            || value.chars().any(|character| {
                character.is_control()
                    || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err(Error::InvalidName);
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    InvalidName,
    InvalidRecord,
    UnsafeDirectory,
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidName => "display name is invalid",
            Self::InvalidRecord => "profile record is invalid or not private",
            Self::UnsafeDirectory => "profile directory must be private and owned by this OS user",
            Self::Io(_) => "profile storage is unavailable",
        })
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rustix::io::Errno> for Error {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}
