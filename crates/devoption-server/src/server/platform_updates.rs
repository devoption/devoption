//! Read-only root-owned observations, independent of private installer journals.
use super::*;
use devoption_ui::{PlatformUpdates, UpgradeNotice};
use rustix::fs::{Mode, OFlags, open, openat};
use std::{
    fs::File,
    io::Read,
    os::unix::fs::MetadataExt,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) async fn home() -> impl IntoResponse {
    let view = tokio::task::spawn_blocking(observation)
        .await
        .unwrap_or_else(|_| unavailable());
    (
        [(CACHE_CONTROL, "no-store")],
        Html(devoption_ui::render_platform_home(&view)),
    )
}

fn unavailable() -> PlatformUpdates {
    PlatformUpdates {
        running_version: env!("CARGO_PKG_VERSION").into(),
        unavailable: true,
        notice: None,
    }
}

fn observation() -> PlatformUpdates {
    // The disposable browser test binary alone can render a named synthetic
    // observation. Production builds have no environment/path override.
    #[cfg(test)]
    if let Ok(phase) = std::env::var("DEVOPTION_BROWSER_UPGRADE")
        && matches!(
            phase.as_str(),
            "not-started" | "healthy" | "failed" | "restored"
        )
    {
        let running = env!("CARGO_PKG_VERSION").to_owned();
        let (previous, candidate) = if phase == "healthy" {
            ("0.1.0-preview.1".into(), running.clone())
        } else {
            (running.clone(), "0.1.0-preview.3".into())
        };
        return PlatformUpdates {
            running_version: running,
            unavailable: false,
            notice: Some(UpgradeNotice {
                previous_version: previous,
                candidate_version: candidate,
                archive_sha256: "a".repeat(64),
                source_commit: "b".repeat(40),
                transaction: "c".repeat(64),
                attempt: 1,
                phase,
                age_seconds: 42,
            }),
        };
    }
    let mut view = unavailable();
    // No host paths or underlying storage errors reach the page.
    if let Ok(notice) = read_notice() {
        view.notice = notice;
        view.unavailable = false;
    }
    view
}

fn directory(file: &File, uid: u32) -> io::Result<()> {
    let info = file.metadata()?;
    if !info.is_dir() || info.uid() != uid || info.mode() & 0o022 != 0 || info.nlink() == 0 {
        return Err(io::Error::other("unsafe upgrade notice directory"));
    }
    Ok(())
}

fn read_notice() -> io::Result<Option<UpgradeNotice>> {
    if !cfg!(target_os = "linux") {
        return Ok(None);
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut parent = File::from(open("/", flags, Mode::empty())?);
    directory(&parent, 0)?;
    for component in ["var", "lib", "devoption-updates"] {
        match openat(&parent, component, flags, Mode::empty()) {
            Ok(fd) => {
                parent = File::from(fd);
                directory(&parent, 0)?;
            }
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
    match openat(
        &parent,
        "notice.json",
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => read_file(File::from(fd), 0).map(Some),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_file(mut file: File, uid: u32) -> io::Result<UpgradeNotice> {
    let info = file.metadata()?;
    if !info.is_file()
        || info.uid() != uid
        || info.mode() & 0o7777 != 0o644
        || info.nlink() != 1
        || info.len() > 8192
    {
        return Err(io::Error::other("unsafe upgrade notice file"));
    }
    let mut raw = Vec::new();
    (&mut file).take(8193).read_to_end(&mut raw)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    parse(&raw, now).ok_or_else(|| io::Error::other("invalid upgrade notice"))
}

fn version(value: &str) -> bool {
    let Some((base, preview)) = value.split_once("-preview.") else {
        return false;
    };
    let number = |s: &str| {
        !s.is_empty()
            && s.len() <= 9
            && s.bytes().all(|b| b.is_ascii_digit())
            && (s == "0" || !s.starts_with('0'))
    };
    value.len() <= 64
        && base.split('.').count() == 3
        && base.split('.').all(number)
        && number(preview)
        && preview != "0"
}

fn parse(raw: &[u8], now: u64) -> Option<UpgradeNotice> {
    if raw.len() > 8192 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    // Canonical bytes also reject duplicate fields and ambiguous encodings.
    let mut canonical = serde_json::to_vec_pretty(&value).ok()?;
    canonical.push(b'\n');
    if canonical != raw
        || value.as_object()?.len() != 10
        || value["schema_version"].as_u64()? != 1
        || value["publication_eligible"].as_bool()?
    {
        return None;
    }
    let text = |key| value[key].as_str().map(str::to_owned);
    let previous_version = text("previous_version")?;
    let candidate_version = text("candidate_version")?;
    if !version(&previous_version)
        || !version(&candidate_version)
        || previous_version == candidate_version
    {
        return None;
    }
    let hex = |key, len| {
        let s = text(key)?;
        (s.len() == len
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then_some(s)
    };
    let checked = value["checked_at"].as_u64()?;
    if checked == 0 || checked > now {
        return None;
    }
    let attempt = value["attempt"].as_u64()?;
    if !(1..=100).contains(&attempt) {
        return None;
    }
    let phase = text("phase")?;
    if ![
        "not-started",
        "prepared",
        "backed-up",
        "activated",
        "healthy",
        "failed",
        "restoring",
        "restored",
    ]
    .contains(&phase.as_str())
    {
        return None;
    }
    Some(UpgradeNotice {
        previous_version,
        candidate_version,
        archive_sha256: hex("candidate_archive_sha256", 64)?,
        source_commit: hex("candidate_source_commit", 40)?,
        transaction: hex("transaction", 64)?,
        attempt,
        phase,
        age_seconds: now - checked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, os::unix::fs::PermissionsExt};

    fn value() -> serde_json::Value {
        serde_json::json!({"schema_version":1,"previous_version":"0.1.0-preview.1","candidate_version":"0.1.0-preview.2","candidate_archive_sha256":"a".repeat(64),"candidate_source_commit":"b".repeat(40),"transaction":"c".repeat(64),"attempt":1,"phase":"not-started","checked_at":100,"publication_eligible":false})
    }
    fn bytes(v: &serde_json::Value) -> Vec<u8> {
        let mut raw = serde_json::to_vec_pretty(v).unwrap();
        raw.push(b'\n');
        raw
    }

    #[test]
    fn strict_observations_reject_ambiguous_untrusted_or_future_results() {
        let v = value();
        assert_eq!(parse(&bytes(&v), 200).unwrap().age_seconds, 100);
        for (key, bad) in [
            ("schema_version", serde_json::json!(true)),
            ("attempt", serde_json::json!(101)),
            ("phase", serde_json::json!("<script>")),
            ("checked_at", serde_json::json!(201)),
            ("publication_eligible", serde_json::json!(true)),
            ("candidate_version", serde_json::json!("0.1.0-preview.1")),
            ("candidate_source_commit", serde_json::json!("x".repeat(40))),
        ] {
            let mut changed = v.clone();
            changed[key] = bad;
            assert!(parse(&bytes(&changed), 200).is_none(), "{key}");
        }
        let mut extra = v.clone();
        extra["command"] = serde_json::json!("ignored?");
        assert!(parse(&bytes(&extra), 200).is_none());
        assert!(parse(&bytes(&v).into_iter().chain(*b" ").collect::<Vec<_>>(), 200).is_none());
        let duplicate = String::from_utf8(bytes(&v))
            .unwrap()
            .replace("\"attempt\": 1,", "\"attempt\": 1,\n  \"attempt\": 2,");
        assert!(parse(duplicate.as_bytes(), 200).is_none());
    }

    #[test]
    fn unsafe_notice_files_are_not_displayed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notice.json");
        let mut file = File::create(&path).unwrap();
        file.write_all(&bytes(&value())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let uid = file.metadata().unwrap().uid();
        assert!(read_file(File::open(&path).unwrap(), uid).is_ok());
        assert!(read_file(File::open(&path).unwrap(), uid + 1).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(read_file(File::open(&path).unwrap(), uid).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::hard_link(&path, root.path().join("link")).unwrap();
        assert!(read_file(File::open(&path).unwrap(), uid).is_err());
    }
}
