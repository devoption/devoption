//! On-demand Git status using private metadata, never source configuration or index writes.
use super::{InspectionError as Error, PermittedRoot, Result, WorktreeObservation, filesystem};
use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_INDEX: usize = 1024 * 1024;
const MAX_OUTPUT: usize = 64 * 1024;
const MAX_RECORDS: usize = 1024;
const DEADLINE: Duration = Duration::from_secs(3);

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeCounts {
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
    pub conflicted: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub changes: ChangeCounts,
}

#[derive(PartialEq, Eq)]
struct Snapshot {
    observed: WorktreeObservation,
    branch: Option<String>,
    commit: Option<String>,
    head: String,
    index: Option<Vec<u8>>,
    exclude: Option<Vec<u8>>,
    config: String,
    objects: PathBuf,
}

impl PermittedRoot {
    /// Read a bounded status observation. Git receives a private copy of bounded
    /// metadata and no source/global configuration; working files and objects are
    /// read in place. This is not an atomic filesystem snapshot or a sandbox.
    pub fn git_status(&self, candidate: &Path) -> Result<GitStatus> {
        let before = self.status_snapshot(candidate)?;
        let config = safe_config(&before.config)?;
        let metadata = tempfile::Builder::new()
            .prefix("devoption-status-")
            .tempdir()
            .map_err(|_| Error::Unavailable)?;
        fs::create_dir(metadata.path().join("objects")).map_err(|_| Error::Unavailable)?;
        fs::create_dir(metadata.path().join("refs")).map_err(|_| Error::Unavailable)?;
        fs::write(metadata.path().join("config"), config).map_err(|_| Error::Unavailable)?;
        let head = before.commit.as_deref().unwrap_or(&before.head);
        fs::write(metadata.path().join("HEAD"), format!("{head}\n"))
            .map_err(|_| Error::Unavailable)?;
        if let Some(index) = &before.index {
            fs::write(metadata.path().join("index"), index).map_err(|_| Error::Unavailable)?;
        }
        if let Some(exclude) = &before.exclude {
            fs::create_dir(metadata.path().join("info")).map_err(|_| Error::Unavailable)?;
            fs::write(metadata.path().join("info/exclude"), exclude)
                .map_err(|_| Error::Unavailable)?;
        }
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OBJECT_DIRECTORY", &before.objects)
            .current_dir(metadata.path())
            .arg("--no-pager")
            .arg("--git-dir")
            .arg(metadata.path())
            .arg("--work-tree")
            .arg(&before.observed.path)
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.attributesFile=/dev/null",
                "-c",
                "protocol.allow=never",
                "status",
                "--porcelain=v1",
                "-z",
                "--no-renames",
                "--untracked-files=normal",
                "--ignore-submodules=all",
            ]);
        let output = run(&mut command, DEADLINE)?;
        let changes = parse(&output)?;
        if self.status_snapshot(candidate)? != before {
            return Err(Error::Changed);
        }
        Ok(GitStatus {
            branch: before.branch,
            commit: before.commit,
            changes,
        })
    }

    fn status_snapshot(&self, candidate: &Path) -> Result<Snapshot> {
        let (observed, git, common) = self.inspect_layout(candidate)?;
        if self
            .optional_status_file(&candidate.join(".gitmodules"), 16 * 1024)?
            .is_some()
            || self
                .optional_status_file(&common.join("shallow"), 16 * 1024)?
                .is_some()
            || self
                .optional_status_file(&common.join("info/attributes"), 16 * 1024)?
                .is_some()
        {
            return Err(Error::UnsupportedLayout);
        }
        let head =
            super::format::line(&self.required(&git.join("HEAD"), super::MAX_PATH)?)?.to_owned();
        let (branch, commit) = if let Some(reference) = head.strip_prefix("ref: ") {
            let branch = reference
                .strip_prefix("refs/heads/")
                .ok_or(Error::InvalidLayout)?
                .to_owned();
            let loose = self.optional_status_file(&common.join(reference), super::MAX_PATH)?;
            let commit = if let Some(bytes) = loose {
                Some(oid(super::format::line(
                    std::str::from_utf8(&bytes).map_err(|_| Error::InvalidLayout)?,
                )?)?)
            } else {
                self.optional_status_file(&common.join("packed-refs"), MAX_INDEX)?
                    .map(|bytes| packed_ref(&bytes, reference))
                    .transpose()?
                    .flatten()
            };
            (Some(branch), commit)
        } else {
            (None, Some(oid(&head)?))
        };
        Ok(Snapshot {
            observed,
            branch,
            commit,
            head,
            index: self.optional_status_file(&git.join("index"), MAX_INDEX)?,
            exclude: self.optional_status_file(&common.join("info/exclude"), 16 * 1024)?,
            config: self.required(&common.join("config"), super::MAX_CONFIG)?,
            objects: self.path.join(common.join("objects")),
        })
    }

    // Missing intermediate metadata directories mean an absent optional file;
    // unsafe/unreadable entries remain errors. Every component uses the root handle.
    fn optional_status_file(&self, path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
        let mut prefix = PathBuf::new();
        for part in path.components() {
            prefix.push(part);
            let Some(file) = self.entry(&prefix)? else {
                return Ok(None);
            };
            if prefix == path {
                return filesystem::bytes(file, limit).map(Some);
            }
            self.directory(&prefix)?;
        }
        Err(Error::InvalidPath)
    }
}

fn oid(value: &str) -> Result<String> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidLayout);
    }
    Ok(value.to_ascii_lowercase())
}

fn packed_ref(bytes: &[u8], wanted: &str) -> Result<Option<String>> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidLayout)?;
    let mut found = None;
    for line in text.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let (hash, name) = line.split_once(' ').ok_or(Error::InvalidLayout)?;
        let hash = oid(hash)?;
        if name == wanted {
            if found.is_some() {
                return Err(Error::InvalidLayout);
            }
            found = Some(hash);
        }
    }
    Ok(found)
}

// Copy only builtin normalization settings that affect status. Executable/filter,
// sparse and external ignore configurations are unsupported, not silently applied.
fn safe_config(source: &str) -> Result<String> {
    let mut section = "";
    let mut result = String::from("[core]\nrepositoryformatversion = 0\nbare = false\n");
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(['#', ';']))
    {
        if line.starts_with('[') {
            section = line
                .trim_matches(['[', ']'])
                .split(' ')
                .next()
                .unwrap_or("");
            if ["filter", "extensions", "submodule"]
                .iter()
                .any(|s| section.eq_ignore_ascii_case(s))
            {
                return Err(Error::UnsupportedLayout);
            }
        } else if section.eq_ignore_ascii_case("core") {
            let (key, value) = line.split_once('=').ok_or(Error::UnsupportedLayout)?;
            let key = key.trim().to_ascii_lowercase();
            if [
                "sparsecheckout",
                "sparsecheckoutcone",
                "excludesfile",
                "attributesfile",
            ]
            .contains(&key.as_str())
            {
                return Err(Error::UnsupportedLayout);
            }
            if [
                "filemode",
                "symlinks",
                "ignorecase",
                "precomposeunicode",
                "autocrlf",
                "eol",
                "safecrlf",
            ]
            .contains(&key.as_str())
            {
                result.push_str(&format!("{key} = {}\n", value.trim()));
            }
        }
    }
    Ok(result)
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(command: &mut Command, duration: Duration) -> Result<Vec<u8>> {
    let mut child = Running(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Error::Unavailable)?,
    );
    let mut stdout = child.0.stdout.take().ok_or(Error::Unavailable)?;
    let flags = rustix::fs::fcntl_getfl(&stdout).map_err(|_| Error::Unavailable)?;
    rustix::fs::fcntl_setfl(&stdout, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(|_| Error::Unavailable)?;
    let deadline = Instant::now() + duration;
    let mut output = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        if Instant::now() >= deadline {
            return Err(Error::LimitExceeded);
        }
        match stdout.read(&mut buffer) {
            Ok(0) => {
                if let Some(status) = child.0.try_wait().map_err(|_| Error::Unavailable)? {
                    return if status.success() {
                        Ok(output)
                    } else {
                        Err(Error::Unavailable)
                    };
                }
            }
            Ok(count) => {
                if output.len() + count > MAX_OUTPUT {
                    return Err(Error::LimitExceeded);
                }
                output.extend_from_slice(&buffer[..count]);
                continue;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error::Unavailable),
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn parse(output: &[u8]) -> Result<ChangeCounts> {
    let mut counts = ChangeCounts::default();
    if output.is_empty() {
        return Ok(counts);
    }
    if !output.ends_with(&[0]) {
        return Err(Error::InvalidLayout);
    }
    for (index, record) in output[..output.len() - 1]
        .split(|byte| *byte == 0)
        .enumerate()
    {
        if index == MAX_RECORDS {
            return Err(Error::LimitExceeded);
        }
        if record.len() < 4 || record[2] != b' ' {
            return Err(Error::InvalidLayout);
        }
        let pair = &record[..2];
        if pair == b"??" {
            counts.untracked += 1;
        } else if [b"DD", b"AU", b"UD", b"UA", b"DU", b"AA", b"UU"]
            .contains(&pair.try_into().map_err(|_| Error::InvalidLayout)?)
        {
            counts.conflicted += 1;
        } else {
            if !pair.iter().all(|byte| b" MADT".contains(byte)) || pair == b"  " {
                return Err(Error::InvalidLayout);
            }
            counts.staged += usize::from(pair[0] != b' ');
            counts.modified += usize::from(pair[1] != b' ');
        }
    }
    Ok(counts)
}
