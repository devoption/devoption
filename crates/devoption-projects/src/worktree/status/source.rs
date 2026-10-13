//! Bounded selected-commit object collection, without configuration/filter execution.
use super::*;
use std::{collections::BTreeSet, os::unix::fs::MetadataExt};

const MAX_OBJECTS: usize = 256;
const MAX_FILES: usize = 128;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GitSourceKind {
    Tree,
    Blob,
}

/// Raw typed payloads; consumers must verify their Git hashes and complete graph.
/// No Debug implementation implicitly logs source bytes.
pub struct GitSourceObject {
    kind: GitSourceKind,
    bytes: Vec<u8>,
}
impl GitSourceObject {
    pub fn kind(&self) -> GitSourceKind {
        self.kind
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
pub struct GitSourceObjects {
    commit: Vec<u8>,
    objects: Vec<GitSourceObject>,
}
impl GitSourceObjects {
    pub fn commit(&self) -> &[u8] {
        &self.commit
    }
    pub fn objects(&self) -> &[GitSourceObject] {
        &self.objects
    }
}

impl PermittedRoot {
    /// Collect exactly a selected current HEAD's source objects, never its history
    /// or the complete object database. A later verifier must validate membership.
    pub fn source_objects(&self, candidate: &Path, selected: &str) -> Result<GitSourceObjects> {
        selected_id(selected)?;
        let before = self.status_snapshot(candidate)?;
        require_selected(&before, selected)?;
        require_clean(self.git_status(candidate)?, selected)?;
        let metadata = before.metadata()?;
        let mut listing = before.command(metadata.path());
        listing.env("GIT_NO_LAZY_FETCH", "1").args([
            "ls-tree",
            "-r",
            "-t",
            "-z",
            "--full-tree",
            selected,
        ]);
        let ids = tree_ids(&run(&mut listing, DEADLINE)?)?;
        let mut requests = vec![
            (selected.to_owned(), "commit"),
            (format!("{selected}^{{tree}}"), "tree"),
        ];
        requests.extend(ids.iter().map(|(id, kind)| (id.clone(), *kind)));
        let path = metadata.path().join("source-requests");
        fs::write(
            &path,
            requests
                .iter()
                .map(|(id, _)| format!("{id}\n"))
                .collect::<String>(),
        )
        .map_err(|_| Error::Unavailable)?;
        let mut check = before.command(metadata.path());
        check
            .env("GIT_NO_LAZY_FETCH", "1")
            .args(["cat-file", "--batch-check"]);
        let headers = run_bounded(
            &mut check,
            DEADLINE,
            MAX_OUTPUT,
            File::open(&path).map_err(|_| Error::Unavailable)?.into(),
        )?;
        let expected = batch_headers(&headers, &requests)?;
        let mut contents = before.command(metadata.path());
        contents
            .env("GIT_NO_LAZY_FETCH", "1")
            .args(["cat-file", "--batch"]);
        let bytes = run_bounded(
            &mut contents,
            DEADLINE,
            MAX_BYTES + MAX_OUTPUT,
            File::open(&path).map_err(|_| Error::Unavailable)?.into(),
        )?;
        let result = batch_contents(&bytes, &expected)?;
        require_clean(self.git_status(candidate)?, selected)?;
        if self.status_snapshot(candidate)? != before {
            return Err(Error::Changed);
        }
        Ok(result)
    }

    /// Compare caller-supplied verified file bytes through the anchored root.
    /// This checks the selected set, not completeness; the snapshot verifier must
    /// supply every committed file. No text/line-ending normalization is applied.
    pub fn check_source_files(
        &self,
        candidate: &Path,
        selected: &str,
        files: &[(&str, &[u8])],
    ) -> Result<()> {
        selected_id(selected)?;
        if files.is_empty() || files.len() > MAX_FILES {
            return Err(Error::LimitExceeded);
        }
        let mut names = BTreeSet::new();
        let mut total = 0usize;
        for (path, bytes) in files {
            files::valid_path(path)?;
            total = total.checked_add(bytes.len()).ok_or(Error::LimitExceeded)?;
            if bytes.len() > MAX_FILE_BYTES || total > MAX_BYTES {
                return Err(Error::LimitExceeded);
            }
            if !names.insert(*path) {
                return Err(Error::InvalidPath);
            }
        }
        let before = self.status_snapshot(candidate)?;
        require_selected(&before, selected)?;
        require_clean(self.git_status(candidate)?, selected)?;
        let mut observations = Vec::new();
        for (relative, expected) in files {
            let path = candidate.join(relative);
            let file = self.entry(&path)?.ok_or(Error::UncommittedSource)?;
            let before = fingerprint(&file)?;
            if before.3 & 0o111 != 0 {
                return Err(Error::UncommittedSource);
            }
            let bytes = filesystem::bytes(
                file.try_clone().map_err(|_| Error::Unavailable)?,
                MAX_FILE_BYTES,
            )?;
            if bytes != *expected {
                return Err(Error::UncommittedSource);
            }
            if fingerprint(&file)? != before {
                return Err(Error::Changed);
            }
            observations.push((path, before));
        }
        require_clean(self.git_status(candidate)?, selected)?;
        if self.status_snapshot(candidate)? != before {
            return Err(Error::Changed);
        }
        for (path, before) in observations {
            let current = self.entry(&path)?.ok_or(Error::Changed)?;
            if fingerprint(&current)? != before {
                return Err(Error::Changed);
            }
        }
        self.check_binding()
    }
}

type Fingerprint = (u64, u64, u64, u32, u32, u64, i64, i64, i64, i64);
fn fingerprint(file: &File) -> Result<Fingerprint> {
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
}

fn selected_id(value: &str) -> Result<()> {
    if value.len() != 40
        || value.bytes().all(|b| b == b'0')
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidLayout);
    }
    Ok(())
}
fn require_selected(snapshot: &Snapshot, selected: &str) -> Result<()> {
    if snapshot.commit.as_deref() != Some(selected) {
        return Err(Error::SelectedCommitMismatch);
    }
    Ok(())
}
fn require_clean(status: GitStatus, selected: &str) -> Result<()> {
    if status.commit.as_deref() != Some(selected) {
        return Err(Error::SelectedCommitMismatch);
    }
    if status.changes != ChangeCounts::default() {
        return Err(Error::UncommittedSource);
    }
    Ok(())
}

fn tree_ids(bytes: &[u8]) -> Result<BTreeSet<(String, &'static str)>> {
    if bytes.is_empty() || !bytes.ends_with(&[0]) {
        return Err(Error::InvalidLayout);
    }
    let mut ids = BTreeSet::new();
    for (index, record) in bytes[..bytes.len() - 1].split(|b| *b == 0).enumerate() {
        if index == MAX_OBJECTS {
            return Err(Error::LimitExceeded);
        }
        let record = std::str::from_utf8(record).map_err(|_| Error::UnsupportedLayout)?;
        let (header, path) = record.split_once('\t').ok_or(Error::InvalidLayout)?;
        files::valid_path(path)?;
        let mut fields = header.split(' ');
        let kind = match (fields.next(), fields.next()) {
            (Some("040000"), Some("tree")) => "tree",
            (Some("100644"), Some("blob")) => "blob",
            _ => return Err(Error::UnsupportedLayout),
        };
        let id = fields.next().ok_or(Error::InvalidLayout)?;
        selected_id(id)?;
        if fields.next().is_some() {
            return Err(Error::InvalidLayout);
        }
        ids.insert((id.to_owned(), kind));
        // The root tree is supplied separately.
        if ids.len() >= MAX_OBJECTS {
            return Err(Error::LimitExceeded);
        }
    }
    Ok(ids)
}

#[derive(PartialEq, Eq)]
struct Header {
    id: String,
    kind: String,
    bytes: usize,
}
fn header(bytes: &[u8]) -> Result<Header> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidLayout)?;
    let mut fields = text.split(' ');
    let id = fields.next().ok_or(Error::InvalidLayout)?;
    selected_id(id)?;
    let kind = fields.next().ok_or(Error::InvalidLayout)?;
    let limit = match kind {
        "tree" | "commit" => MAX_OUTPUT,
        "blob" => MAX_FILE_BYTES,
        _ => return Err(Error::UnsupportedLayout),
    };
    let size = fields.next().ok_or(Error::InvalidLayout)?;
    if size.is_empty() || !size.bytes().all(|b| b.is_ascii_digit()) || fields.next().is_some() {
        return Err(Error::InvalidLayout);
    }
    let bytes = size.parse::<usize>().map_err(|_| Error::LimitExceeded)?;
    if bytes > limit {
        return Err(Error::LimitExceeded);
    }
    Ok(Header {
        id: id.into(),
        kind: kind.into(),
        bytes,
    })
}

fn batch_headers(bytes: &[u8], requests: &[(String, &str)]) -> Result<Vec<Header>> {
    if !bytes.ends_with(b"\n") {
        return Err(Error::InvalidLayout);
    }
    let lines: Vec<_> = bytes[..bytes.len() - 1].split(|b| *b == b'\n').collect();
    if lines.len() != requests.len() {
        return Err(Error::InvalidLayout);
    }
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    let mut total = 0usize;
    for (line, (request, kind)) in lines.into_iter().zip(requests) {
        let value = header(line)?;
        if value.kind != *kind
            || (request.len() == 40 && value.id != *request)
            || !seen.insert(value.id.clone())
        {
            return Err(Error::InvalidLayout);
        }
        total = total.checked_add(value.bytes).ok_or(Error::LimitExceeded)?;
        if total > MAX_BYTES {
            return Err(Error::LimitExceeded);
        }
        result.push(value);
    }
    Ok(result)
}

fn batch_contents(mut bytes: &[u8], expected: &[Header]) -> Result<GitSourceObjects> {
    let mut commit = Vec::new();
    let mut objects = Vec::new();
    for (index, wanted) in expected.iter().enumerate() {
        let end = bytes
            .iter()
            .position(|b| *b == b'\n')
            .ok_or(Error::InvalidLayout)?;
        if header(&bytes[..end])? != *wanted {
            return Err(Error::Changed);
        }
        bytes = &bytes[end + 1..];
        let payload = bytes.get(..wanted.bytes).ok_or(Error::InvalidLayout)?;
        if bytes.get(wanted.bytes) != Some(&b'\n') {
            return Err(Error::InvalidLayout);
        }
        if index == 0 {
            commit = payload.to_vec();
        } else {
            let kind = match wanted.kind.as_str() {
                "tree" => GitSourceKind::Tree,
                "blob" => GitSourceKind::Blob,
                _ => return Err(Error::InvalidLayout),
            };
            objects.push(GitSourceObject {
                kind,
                bytes: payload.to_vec(),
            });
        }
        bytes = &bytes[wanted.bytes + 1..];
    }
    if !bytes.is_empty() {
        return Err(Error::InvalidLayout);
    }
    Ok(GitSourceObjects { commit, objects })
}

#[cfg(test)]
mod tests;
