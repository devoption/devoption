use super::{InspectionError as Error, Result};

pub(super) fn line(value: &str) -> Result<&str> {
    let value = value.strip_suffix('\n').unwrap_or(value);
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(Error::InvalidLayout);
    }
    Ok(value)
}

pub(super) fn head(value: &str) -> Result<()> {
    let value = line(value)?;
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }
    let name = value
        .strip_prefix("ref: refs/heads/")
        .ok_or(Error::InvalidLayout)?;
    if name.is_empty()
        || name.contains("..")
        || name.contains("@{")
        || name.ends_with('.')
        || name
            .bytes()
            .any(|byte| byte <= b' ' || byte == 127 || b"~^:?*[\\".contains(&byte))
        || name
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
    {
        return Err(Error::InvalidLayout);
    }
    Ok(())
}

// Deliberately a small, fail-closed subset, not a general Git config evaluator.
// No includes, escapes, continuation, extensions, worktree overrides or bare
// repositories. Unknown single-line values are never interpreted or followed.
pub(super) fn config(value: &str) -> Result<()> {
    let mut section = String::new();
    let mut version = false;
    let mut bare = false;
    for raw in value.lines() {
        if raw.chars().any(|c| c.is_control() && c != '\t') || raw.contains('\\') {
            return Err(Error::UnsupportedLayout);
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            let inner = line
                .strip_prefix('[')
                .and_then(|v| v.strip_suffix(']'))
                .ok_or(Error::InvalidLayout)?;
            let (name, subsection) = inner
                .split_once(' ')
                .map_or((inner, None), |(name, rest)| (name, Some(rest)));
            if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(Error::UnsupportedLayout);
            }
            if let Some(subsection) = subsection {
                let value = subsection
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .ok_or(Error::UnsupportedLayout)?;
                if value.is_empty() || value.contains('"') {
                    return Err(Error::UnsupportedLayout);
                }
            }
            section = name.to_ascii_lowercase();
            if ["include", "includeif", "extensions"].contains(&section.as_str())
                || (section == "core" && subsection.is_some())
            {
                return Err(Error::UnsupportedLayout);
            }
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(Error::UnsupportedLayout)?;
        if value.contains('"') {
            return Err(Error::UnsupportedLayout);
        }
        let key = key.trim().to_ascii_lowercase();
        if section.is_empty()
            || !key.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Error::InvalidLayout);
        }
        if section == "core" {
            match key.as_str() {
                "repositoryformatversion" => {
                    if version || value.trim() != "0" {
                        return Err(Error::UnsupportedLayout);
                    }
                    version = true;
                }
                "bare" => {
                    if bare || !value.trim().eq_ignore_ascii_case("false") {
                        return Err(Error::UnsupportedLayout);
                    }
                    bare = true;
                }
                "worktree" => return Err(Error::UnsupportedLayout),
                _ => {}
            }
        }
    }
    if !version || !bare {
        return Err(Error::InvalidLayout);
    }
    Ok(())
}
