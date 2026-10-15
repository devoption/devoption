use super::*;
use crate::{BuildPlan, review_context::Input};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    sync::Arc,
};

struct Fixture {
    _temporary: tempfile::TempDir,
    bin: PathBuf,
    plan: BuildPlan,
    input: Input,
}
impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let bin = temporary.path().canonicalize().unwrap().join("bin");
        fs::create_dir(&bin).unwrap();
        let cargo = bin.join("cargo");
        let rustc = bin.join("rustc");
        // Inert bytes with executable metadata; never launched as a program.
        for (path, bytes) in [
            (&cargo, b"inert cargo fixture".as_slice()),
            (&rustc, b"inert rustc fixture".as_slice()),
        ] {
            fs::write(path, bytes).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut plan = crate::review_context::tests::plan();
        plan.0.cargo_executable = cargo.to_str().unwrap().into();
        for entry in &mut plan.0.environment {
            if entry.name == "RUSTC" {
                entry.value = rustc.to_str().unwrap().into();
            }
            if entry.name == "PATH" {
                entry.value = bin.to_str().unwrap().into();
            }
        }
        let mut input = crate::review_context::tests::input(&plan);
        input.cargo.sha256 = devoption_registry::Sha256::digest(b"inert cargo fixture");
        input.rustc.path = rustc.to_str().unwrap().into();
        input.rustc.sha256 = devoption_registry::Sha256::digest(b"inert rustc fixture");
        input.execution_uid = geteuid().as_raw();
        Self {
            _temporary: temporary,
            bin,
            plan,
            input,
        }
    }
    fn context(&self) -> ReviewContext {
        ReviewContext::validate(&self.plan, self.input.clone()).unwrap()
    }
    fn cargo(&self) -> PathBuf {
        self.bin.join("cargo")
    }
}

#[test]
fn matching_bytes_can_be_rechecked_concurrently_without_changes_or_execution() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.cargo()).unwrap();
    let pinned = Arc::new(ReviewedExecutables::open(&fixture.context()).unwrap());
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let pinned = Arc::clone(&pinned);
            std::thread::spawn(move || pinned.revalidate())
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap(), Ok(()));
    }
    drop(pinned);
    assert_eq!(fs::read(fixture.cargo()).unwrap(), before);
    assert_eq!(fs::read_dir(&fixture.bin).unwrap().count(), 2);
}

#[test]
fn hash_mismatch_and_wrong_execution_account_fail_closed() {
    let mut fixture = Fixture::new();
    fixture.input.cargo.sha256 = devoption_registry::Sha256::digest(b"different");
    assert_eq!(
        ReviewedExecutables::open(&fixture.context()).err(),
        Some(Error::HashMismatch)
    );
    fixture.input.execution_uid += 1;
    assert_eq!(
        ReviewedExecutables::open(&fixture.context()).err(),
        Some(Error::UnsupportedOwner)
    );
}

#[test]
fn empty_oversized_nonexecutable_writable_and_privileged_files_are_rejected() {
    for case in 0..7 {
        let fixture = Fixture::new();
        match case {
            0 => fs::write(fixture.cargo(), b"").unwrap(),
            1 => File::options()
                .write(true)
                .open(fixture.cargo())
                .unwrap()
                .set_len(MAX_EXECUTABLE_BYTES + 1)
                .unwrap(),
            _ => fs::set_permissions(
                fixture.cargo(),
                fs::Permissions::from_mode(match case {
                    2 => 0o600,
                    3 => 0o720,
                    4 => 0o702,
                    5 => 0o4700,
                    _ => 0o2700,
                }),
            )
            .unwrap(),
        }
        assert_eq!(
            ReviewedExecutables::open(&fixture.context()).err(),
            Some(if case == 1 {
                Error::TooLarge
            } else {
                Error::UnsafeFile
            })
        );
    }
}

#[test]
fn symlinks_hardlinks_and_nonregular_files_are_rejected() {
    for case in 0..4 {
        let fixture = Fixture::new();
        match case {
            0 => {
                fs::rename(fixture.cargo(), fixture.bin.join("preserved")).unwrap();
                symlink(fixture.bin.join("preserved"), fixture.cargo()).unwrap();
            }
            1 => {
                let renamed = fixture.bin.with_file_name("preserved-bin");
                fs::rename(&fixture.bin, &renamed).unwrap();
                symlink(&renamed, &fixture.bin).unwrap();
            }
            2 => fs::hard_link(fixture.cargo(), fixture.bin.join("alias")).unwrap(),
            _ => {
                fs::remove_file(fixture.cargo()).unwrap();
                fs::create_dir(fixture.cargo()).unwrap();
            }
        }
        assert!(ReviewedExecutables::open(&fixture.context()).is_err());
    }
}

#[test]
fn retained_files_detect_replacement_rename_unlink_content_and_mode_changes() {
    for case in 0..6 {
        let fixture = Fixture::new();
        let pinned = ReviewedExecutables::open(&fixture.context()).unwrap();
        match case {
            0 => {
                fs::rename(fixture.cargo(), fixture.bin.join("preserved")).unwrap();
                fs::write(fixture.cargo(), b"inert cargo fixture").unwrap();
                fs::set_permissions(fixture.cargo(), fs::Permissions::from_mode(0o700)).unwrap();
            }
            1 => fs::rename(&fixture.bin, fixture.bin.with_file_name("preserved-bin")).unwrap(),
            2 => fs::remove_file(fixture.cargo()).unwrap(),
            3 => fs::write(fixture.cargo(), b"changed cargo bytes").unwrap(),
            4 => fs::set_permissions(fixture.cargo(), fs::Permissions::from_mode(0o500)).unwrap(),
            _ => {
                fs::remove_file(fixture.cargo()).unwrap();
                symlink(fixture.bin.join("rustc"), fixture.cargo()).unwrap();
            }
        }
        assert!(pinned.revalidate().is_err());
        drop(pinned);
        if case == 0 {
            assert!(fixture.bin.join("preserved").is_file());
        }
    }
}

#[test]
fn streaming_hash_has_an_inclusive_bound_and_detects_growth() {
    let fixture = Fixture::new();
    fs::write(fixture.cargo(), b"abc").unwrap();
    let file = open_file(fixture.cargo().to_str().unwrap()).unwrap();
    let uid = geteuid().as_raw();
    assert_eq!(
        hash(&file, uid, 3, || {}).unwrap(),
        *devoption_registry::Sha256::digest(b"abc").bytes()
    );
    assert_eq!(hash(&file, uid, 2, || {}).err(), Some(Error::TooLarge));
    assert_eq!(
        hash(&file, uid, 3, || {
            fs::write(fixture.cargo(), b"abcd").unwrap();
        })
        .err(),
        Some(Error::TooLarge)
    );
}

#[test]
fn changes_during_chunked_reads_do_not_yield_verified_hashes() {
    for case in 0..3 {
        let fixture = Fixture::new();
        fs::write(fixture.cargo(), vec![b'a'; 128 * 1024]).unwrap();
        let file = open_file(fixture.cargo().to_str().unwrap()).unwrap();
        let mut changed = false;
        let result = hash(&file, geteuid().as_raw(), 256 * 1024, || {
            if changed {
                return;
            }
            changed = true;
            match case {
                0 => fs::write(fixture.cargo(), vec![b'b'; 128 * 1024]).unwrap(),
                1 => fs::write(fixture.cargo(), b"short").unwrap(),
                _ => {
                    fs::set_permissions(fixture.cargo(), fs::Permissions::from_mode(0o500)).unwrap()
                }
            }
        });
        assert_eq!(result.err(), Some(Error::Changed));
        fs::set_permissions(fixture.cargo(), fs::Permissions::from_mode(0o700)).unwrap();
    }
}

#[test]
fn io_boundary_rejects_unbounded_or_ambiguous_paths() {
    for invalid in [
        "relative",
        "/",
        "/a/../cargo",
        "/a//cargo",
        "/a/./cargo",
        "/a/\ncargo",
    ] {
        assert_eq!(open_file(invalid).err(), Some(Error::InvalidPath));
    }
    assert_eq!(
        open_file(&format!("/{}cargo", "a/".repeat(64))).err(),
        Some(Error::InvalidPath)
    );
    assert_eq!(
        open_file(&format!("/{}", "a".repeat(4096))).err(),
        Some(Error::InvalidPath)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn fifo_open_is_nonblocking_and_rejected_before_reading() {
    use std::os::unix::fs::FileTypeExt;
    let fixture = Fixture::new();
    fs::remove_file(fixture.cargo()).unwrap();
    rustix::fs::mkfifoat(rustix::fs::CWD, fixture.cargo(), Mode::RUSR | Mode::WUSR).unwrap();
    assert_eq!(
        ReviewedExecutables::open(&fixture.context()).err(),
        Some(Error::UnsafeFile)
    );
    assert!(
        fs::symlink_metadata(fixture.cargo())
            .unwrap()
            .file_type()
            .is_fifo()
    );
}
