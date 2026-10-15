use super::*;
use crate::{BuildPlan, review_context::Input};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

struct Fixture {
    _temporary: tempfile::TempDir,
    base: PathBuf,
    work: PathBuf,
    plan: BuildPlan,
    input: Input,
}
impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let base = temporary.path().canonicalize().unwrap();
        let work = base.join("work");
        fs::create_dir(&work).unwrap();
        fs::set_permissions(&work, fs::Permissions::from_mode(0o700)).unwrap();
        let mut plan = crate::review_context::tests::plan();
        plan.0.work_directory = work.to_str().unwrap().into();
        let mut input = crate::review_context::tests::input(&plan);
        input.execution_uid = geteuid().as_raw();
        for (i, directory) in input.directories.iter_mut().enumerate() {
            let path = if i == 0 {
                work.clone()
            } else {
                work.join(format!("role-{i}"))
            };
            if i != 0 {
                fs::create_dir(&path).unwrap();
            }
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            let metadata = fs::metadata(&path).unwrap();
            directory.path = path.to_str().unwrap().into();
            directory.device = metadata.dev();
            directory.inode = metadata.ino();
            directory.owner_uid = metadata.uid();
            directory.mode = 0o700;
            let variable = match directory.role {
                DirectoryRole::Home => Some("HOME"),
                DirectoryRole::CargoHome => Some("CARGO_HOME"),
                DirectoryRole::Target => Some("CARGO_TARGET_DIR"),
                DirectoryRole::Temporary => Some("TMPDIR"),
                _ => None,
            };
            if let Some(name) = variable {
                plan.0
                    .environment
                    .iter_mut()
                    .find(|e| e.name == name)
                    .unwrap()
                    .value = directory.path.clone();
            }
        }
        Self {
            _temporary: temporary,
            base,
            work,
            plan,
            input,
        }
    }
    fn context(&self) -> ReviewContext {
        ReviewContext::validate(&self.plan, self.input.clone()).unwrap()
    }
    fn path(&self, index: usize) -> PathBuf {
        PathBuf::from(&self.input.directories[index].path)
    }
}

#[test]
fn successful_open_recheck_and_drop_preserve_existing_tree() {
    let fixture = Fixture::new();
    let marker = fixture.path(1).join("keep");
    fs::write(&marker, b"unchanged fixture bytes").unwrap();
    let before: Vec<_> = fixture
        .input
        .directories
        .iter()
        .map(|d| {
            let m = fs::metadata(&d.path).unwrap();
            (m.dev(), m.ino(), m.mode())
        })
        .collect();
    let pinned = ReviewedDirectories::open(&fixture.context()).unwrap();
    assert_eq!(pinned.revalidate(), Ok(()));
    assert_eq!(pinned.handles.len(), 7);
    drop(pinned);
    for (directory, original) in fixture.input.directories.iter().zip(before) {
        let m = fs::metadata(&directory.path).unwrap();
        assert_eq!((m.dev(), m.ino(), m.mode()), original);
    }
    assert_eq!(fs::read(marker).unwrap(), b"unchanged fixture bytes");
    assert_eq!(fs::read_dir(&fixture.work).unwrap().count(), 6);
}

#[test]
fn claimed_inode_device_mode_and_account_cannot_replace_actual_metadata() {
    for case in 0..5 {
        let mut fixture = Fixture::new();
        match case {
            0 => fixture.input.directories[1].inode += 100_000,
            1 => fixture.input.directories[1].device += 1,
            2 => fixture.input.directories[1].mode = 0o500,
            3 => fixture.input.directories[1].owner_uid += 1,
            _ => fixture.input.execution_uid += 1,
        }
        let error = ReviewedDirectories::open(&fixture.context()).err().unwrap();
        assert_eq!(
            error,
            if case >= 3 {
                Error::UnsupportedOwner
            } else {
                Error::Changed
            }
        );
    }
}

#[test]
fn separate_roots_nested_roles_and_depth_overflow_are_unsupported() {
    for case in 0..3 {
        let mut fixture = Fixture::new();
        // Source has no corresponding environment variable; keep all other
        // metadata valid to isolate the layout restriction.
        fixture.input.directories[1].path = match case {
            0 => fixture.base.join("outside").to_str().unwrap().into(),
            1 => format!("{}/nested", fixture.input.directories[2].path),
            _ => format!("{}/{}", fixture.work.display(), vec!["deep"; 65].join("/")),
        };
        assert_eq!(
            ReviewedDirectories::open(&fixture.context()).err(),
            Some(Error::UnsupportedLayout)
        );
    }
}

#[test]
fn symlink_leaf_and_ancestor_are_never_followed() {
    for ancestor in [false, true] {
        let fixture = Fixture::new();
        let original = if ancestor {
            fixture.work.clone()
        } else {
            fixture.path(1)
        };
        let renamed = fixture.base.join("preserved");
        fs::rename(&original, &renamed).unwrap();
        symlink(&renamed, &original).unwrap();
        assert_eq!(
            ReviewedDirectories::open(&fixture.context()).err(),
            Some(Error::Unavailable)
        );
        assert!(renamed.is_dir());
    }
}

#[test]
fn recheck_detects_root_leaf_replacement_unlink_and_permission_change() {
    for case in 0..5 {
        let fixture = Fixture::new();
        let pinned = ReviewedDirectories::open(&fixture.context()).unwrap();
        match case {
            0 | 1 => {
                let old = if case == 0 {
                    fixture.work.clone()
                } else {
                    fixture.path(1)
                };
                fs::rename(&old, fixture.base.join("preserved")).unwrap();
                fs::create_dir(&old).unwrap();
                fs::set_permissions(&old, fs::Permissions::from_mode(0o700)).unwrap();
            }
            2 => fs::remove_dir(fixture.path(1)).unwrap(),
            3 => fs::set_permissions(fixture.path(1), fs::Permissions::from_mode(0o755)).unwrap(),
            _ => {
                fs::rename(fixture.path(1), fixture.base.join("preserved")).unwrap();
                symlink(fixture.base.join("preserved"), fixture.path(1)).unwrap();
            }
        }
        assert!(pinned.revalidate().is_err());
        drop(pinned);
        if matches!(case, 0 | 1 | 4) {
            assert!(fixture.base.join("preserved").is_dir());
        }
    }
}

#[test]
fn unsafe_intermediate_components_and_device_boundary_fail_closed() {
    let mut fixture = Fixture::new();
    let intermediate = fixture.work.join("intermediate");
    fs::create_dir(&intermediate).unwrap();
    fs::set_permissions(&intermediate, fs::Permissions::from_mode(0o755)).unwrap();
    let nested = intermediate.join("source");
    fs::rename(fixture.path(1), &nested).unwrap();
    fixture.input.directories[1].path = nested.to_str().unwrap().into();
    assert_eq!(
        ReviewedDirectories::open(&fixture.context()).err(),
        Some(Error::Changed)
    );
    fs::set_permissions(&intermediate, fs::Permissions::from_mode(0o700)).unwrap();
    let pinned = ReviewedDirectories::open(&fixture.context()).unwrap();
    assert_eq!(
        descendant(
            &pinned.handles[0],
            "intermediate/source",
            fixture.input.directories[0].device + 1,
            pinned.uid
        )
        .err(),
        Some(Error::Changed)
    );
    // No privileged mount is created: this checks the device comparison using
    // a deliberately wrong allowed device, not a real cross-mount integration.
    fs::set_permissions(&intermediate, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(pinned.revalidate(), Err(Error::Changed));
}

#[test]
fn missing_and_regular_file_roles_are_rejected_without_modification() {
    for regular in [false, true] {
        let fixture = Fixture::new();
        fs::remove_dir(fixture.path(1)).unwrap();
        if regular {
            fs::write(fixture.path(1), b"keep").unwrap();
        }
        assert_eq!(
            ReviewedDirectories::open(&fixture.context()).err(),
            Some(Error::Unavailable)
        );
        if regular {
            assert_eq!(fs::read(fixture.path(1)).unwrap(), b"keep");
        }
    }
}

#[test]
fn read_only_source_mode_is_checked_exactly() {
    let mut fixture = Fixture::new();
    fs::set_permissions(fixture.path(1), fs::Permissions::from_mode(0o500)).unwrap();
    fixture.input.directories[1].mode = 0o500;
    let pinned = ReviewedDirectories::open(&fixture.context()).unwrap();
    assert_eq!(pinned.revalidate(), Ok(()));
    fs::set_permissions(fixture.path(1), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(pinned.revalidate(), Err(Error::Changed));
}

#[cfg(target_os = "linux")]
#[test]
fn fifo_role_is_rejected_without_opening_it_as_a_stream() {
    use std::os::unix::fs::FileTypeExt;
    let fixture = Fixture::new();
    fs::remove_dir(fixture.path(1)).unwrap();
    rustix::fs::mkfifoat(rustix::fs::CWD, fixture.path(1), Mode::RUSR | Mode::WUSR).unwrap();
    assert_eq!(
        ReviewedDirectories::open(&fixture.context()).err(),
        Some(Error::Unavailable)
    );
    assert!(
        fs::symlink_metadata(fixture.path(1))
            .unwrap()
            .file_type()
            .is_fifo()
    );
}
