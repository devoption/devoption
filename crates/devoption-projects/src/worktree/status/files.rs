//! Tracked working-tree text, never a committed snapshot or execution input.
use super::*;
use std::os::unix::fs::MetadataExt;

const MAX_FILES: usize = 256;
const MAX_FILE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackedFile {
    pub path: String,
    pub regular: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileContent {
    Text(String),
    Binary,
    TooLarge,
    Unsupported,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryFiles {
    pub entries: Vec<TrackedFile>,
    pub selected: Option<(String, FileContent)>,
}

impl PermittedRoot {
    /// Read paths from a private bounded index copy, then only a selected tracked
    /// file through the anchored root. No source objects/configuration are used
    /// by Git. External writers remain trusted; this is not an atomic snapshot.
    pub fn repository_files(
        &self,
        candidate: &Path,
        selected: Option<&str>,
    ) -> Result<RepositoryFiles> {
        if let Some(path) = selected {
            valid_path(path)?;
        }
        let before = self.status_snapshot(candidate)?;
        let metadata = before.metadata()?;
        let mut command = before.command(metadata.path());
        // ls-files needs the copied index only. Sparse entries remain unexpanded;
        // object lookup cannot reach the source repository or external alternates.
        command
            .env("GIT_OBJECT_DIRECTORY", metadata.path().join("objects"))
            .args(["ls-files", "--cached", "--stage", "--sparse", "-z"]);
        let entries = parse(&run(&mut command, DEADLINE)?)?;
        let selected = selected
            .map(|path| {
                let entry = entries
                    .iter()
                    .find(|entry| entry.path == path)
                    .ok_or(Error::InvalidPath)?;
                let content = if entry.regular {
                    self.preview_file(&candidate.join(path))?
                } else {
                    FileContent::Unsupported
                };
                Ok((path.to_owned(), content))
            })
            .transpose()?;
        if self.status_snapshot(candidate)? != before {
            return Err(Error::Changed);
        }
        Ok(RepositoryFiles { entries, selected })
    }

    fn preview_file(&self, path: &Path) -> Result<FileContent> {
        let Some(file) = self.entry(path).ok().flatten() else {
            return Ok(FileContent::Unavailable);
        };
        let fingerprint = |file: &File| -> Result<_> {
            let m = file.metadata().map_err(|_| Error::Unavailable)?;
            Ok((
                m.dev(),
                m.ino(),
                m.len(),
                m.mode(),
                m.uid(),
                m.nlink(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
            ))
        };
        let before = fingerprint(&file)?;
        let bytes = match filesystem::bytes(
            file.try_clone().map_err(|_| Error::Unavailable)?,
            MAX_FILE_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(Error::LimitExceeded) => return Ok(FileContent::TooLarge),
            Err(Error::Changed) => return Err(Error::Changed),
            Err(_) => return Ok(FileContent::Unavailable),
        };
        let current = self.entry(path)?.ok_or(Error::Changed)?;
        if fingerprint(&file)? != before || fingerprint(&current)? != before {
            return Err(Error::Changed);
        }
        Ok(match String::from_utf8(bytes) {
            Ok(text)
                if !text
                    .chars()
                    .any(|c| (c.is_control() && !matches!(c, '\n' | '\r' | '\t')) || bidi(c)) =>
            {
                FileContent::Text(text)
            }
            _ => FileContent::Binary,
        })
    }
}

fn bidi(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn valid_path(path: &str) -> Result<()> {
    filesystem::relative(Path::new(path))?;
    if path.len() > 1024 || path.split('/').count() > 16 {
        return Err(Error::LimitExceeded);
    }
    if path.contains('\\')
        || path.chars().any(bidi)
        || path.split('/').any(|p| p.eq_ignore_ascii_case(".git"))
    {
        return Err(Error::InvalidPath);
    }
    Ok(())
}

fn parse(bytes: &[u8]) -> Result<Vec<TrackedFile>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.ends_with(&[0]) {
        return Err(Error::InvalidLayout);
    }
    let mut entries = std::collections::BTreeMap::new();
    for record in bytes[..bytes.len() - 1].split(|b| *b == 0) {
        if entries.len() == MAX_FILES {
            return Err(Error::LimitExceeded);
        }
        let record = std::str::from_utf8(record).map_err(|_| Error::UnsupportedLayout)?;
        let (header, path) = record.split_once('\t').ok_or(Error::InvalidLayout)?;
        valid_path(path)?;
        let mut parts = header.split(' ');
        let mode = parts.next().ok_or(Error::InvalidLayout)?;
        let hash = parts.next().ok_or(Error::InvalidLayout)?;
        if hash.len() != 40
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || parts.next() != Some("0")
            || parts.next().is_some()
        {
            return Err(Error::UnsupportedLayout);
        }
        let regular = match mode {
            "100644" | "100755" => true,
            "120000" | "160000" | "040000" => false,
            _ => return Err(Error::UnsupportedLayout),
        };
        if entries.insert(path.to_owned(), regular).is_some() {
            return Err(Error::InvalidLayout);
        }
    }
    Ok(entries
        .into_iter()
        .map(|(path, regular)| TrackedFile { path, regular })
        .collect())
}

#[cfg(test)]
mod tests;
