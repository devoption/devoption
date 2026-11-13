//! Deliberately narrow regular-file ustar/GNU-header reader; never extracts to disk.
use super::*;
use flate2::bufread::GzDecoder;
use std::io::Read;

pub(in crate::library) fn read(
    bytes: &[u8],
    package: &LockedPackage,
    remaining: &mut usize,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut decoder = GzDecoder::new(bytes);
    let mut tar = Vec::new();
    (&mut decoder)
        .take(MAX_PACKAGE_TAR_BYTES.min(*remaining) as u64 + 1)
        .read_to_end(&mut tar)
        .map_err(|_| Error::Integrity)?;
    if tar.len() > MAX_PACKAGE_TAR_BYTES.min(*remaining) {
        return Err(Error::Capacity);
    }
    if !decoder.into_inner().is_empty() {
        return Err(Error::Invalid);
    }
    if tar.len() < 1024 || !tar.len().is_multiple_of(512) {
        return Err(Error::Integrity);
    }
    let root = format!("{}-{}", package.name, package.version);
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut explicit = BTreeSet::new();
    let mut spelling = BTreeMap::new();
    let mut offset = 0;
    while offset + 512 <= tar.len() {
        let header = &tar[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            if tar.len() - offset < 1024 || tar[offset..].iter().any(|b| *b != 0) {
                return Err(Error::Integrity);
            }
            if files.is_empty() {
                return Err(Error::Invalid);
            }
            *remaining -= tar.len();
            return Ok(files);
        }
        let checksum: usize = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    usize::from(*b)
                }
            })
            .sum();
        if number(&header[148..156])? != checksum {
            return Err(Error::Integrity);
        }
        let prefix = match &header[257..265] {
            b"ustar\x0000" => {
                if header[500..].iter().any(|b| *b != 0) {
                    return Err(Error::Invalid);
                }
                text(&header[345..500])?
            }
            b"ustar  \x00" if header[345..].iter().all(|b| *b == 0) => "",
            _ => return Err(Error::Invalid),
        };
        if header[157..257].iter().any(|b| *b != 0) {
            return Err(Error::Invalid);
        }
        let name = text(&header[..100])?;
        let full = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        let directory = header[156] == b'5';
        if !directory && !matches!(header[156], 0 | b'0') {
            return Err(Error::Invalid);
        }
        let full = if directory {
            full.strip_suffix('/').unwrap_or(&full)
        } else {
            &full
        };
        let relative = if full == root && directory {
            if !explicit.insert(String::new()) {
                return Err(Error::Invalid);
            }
            ""
        } else {
            full.strip_prefix(&format!("{root}/"))
                .ok_or(Error::Invalid)?
        };
        if relative.is_empty() && full != root {
            return Err(Error::Invalid);
        }
        if !relative.is_empty() {
            safe_path(relative)?;
            if relative.split('/').any(|p| {
                [".cargo", ".git", ".cargo-checksum.json"]
                    .iter()
                    .any(|r| p.eq_ignore_ascii_case(r))
            }) {
                return Err(Error::Invalid);
            }
            let parts: Vec<_> = relative.split('/').collect();
            for n in 1..=parts.len() {
                let path = parts[..n].join("/");
                let key = path.to_ascii_lowercase();
                if spelling.get(&key).is_some_and(|old| old != &path) {
                    return Err(Error::Invalid);
                }
                spelling.insert(key, path.clone());
                if spelling.len() > MAX_PACKAGE_ENTRIES {
                    return Err(Error::Capacity);
                }
                if n < parts.len() {
                    if files.contains_key(&path) {
                        return Err(Error::Invalid);
                    }
                    directories.insert(path);
                }
            }
            if files.contains_key(relative) || (!directory && directories.contains(relative)) {
                return Err(Error::Invalid);
            }
            if directory {
                if !explicit.insert(relative.to_owned()) {
                    return Err(Error::Invalid);
                }
                directories.insert(relative.to_owned());
            }
        }
        let size = number(&header[124..136])?;
        if directory && size != 0 {
            return Err(Error::Invalid);
        }
        let start = offset + 512;
        let end = start.checked_add(size).ok_or(Error::Capacity)?;
        let padded = start
            .checked_add(size.checked_add(511).ok_or(Error::Capacity)? / 512 * 512)
            .ok_or(Error::Capacity)?;
        if end > tar.len() || padded > tar.len() || tar[end..padded].iter().any(|b| *b != 0) {
            return Err(Error::Integrity);
        }
        if !directory {
            if relative.is_empty() {
                return Err(Error::Invalid);
            }
            files.insert(relative.to_owned(), tar[start..end].to_vec());
        }
        offset = padded;
    }
    Err(Error::Integrity)
}
fn text(bytes: &[u8]) -> Result<&str, Error> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    if bytes[end..].iter().any(|b| *b != 0) {
        return Err(Error::Invalid);
    }
    std::str::from_utf8(&bytes[..end]).map_err(|_| Error::Invalid)
}
fn number(bytes: &[u8]) -> Result<usize, Error> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid)?
        .trim_matches(['\0', ' ']);
    if value.is_empty() || !value.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return Err(Error::Invalid);
    }
    usize::from_str_radix(value, 8).map_err(|_| Error::Capacity)
}
