use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
const VALUE: &str = "fixture-secret-never-render-this-value";
const OWNER: [u8; 16] = [7; 16];
const PROJECT: ProjectId = ProjectId::from_bytes([8; 16]);
const ENV: Environment = Environment::Production;
struct Fixture {
    _temp: tempfile::TempDir,
    data: std::path::PathBuf,
    key: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let data = base.join("data");
        fs::create_dir(&data).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
        let key = base.join("fixture.key");
        fs::write(&key, [0xa5; 32]).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        Self {
            _temp: temp,
            data,
            key,
        }
    }
    fn store(&self) -> Store {
        Store::open(File::open(&self.data).unwrap(), &self.key, OWNER, &[]).unwrap()
    }
    fn file(&self) -> std::path::PathBuf {
        self.data.join(Store::filename(PROJECT, ENV))
    }
}
fn value() -> Zeroizing<String> {
    Zeroizing::new(VALUE.into())
}
#[test]
fn encrypted_roundtrip_is_scoped_restartable_and_values_never_enter_metadata() {
    let f = Fixture::new();
    let s = f.store();
    s.set(PROJECT, ENV, "SERVICE_KEY", value(), 0).unwrap();
    let first = fs::read(f.file()).unwrap();
    assert!(!first.windows(VALUE.len()).any(|w| w == VALUE.as_bytes()));
    assert_eq!(fs::metadata(f.file()).unwrap().mode() & 0o777, 0o600);
    assert_eq!(
        s.load(PROJECT, ENV).unwrap().entries[0].value.as_str(),
        VALUE
    );
    assert!(
        s.list(PROJECT, Environment::Development)
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(
        s.list(ProjectId::from_bytes([9; 16]), ENV)
            .unwrap()
            .entries
            .is_empty()
    );
    let list = s.list(PROJECT, ENV).unwrap();
    assert_eq!(list.entries[0].name, "SERVICE_KEY");
    assert_eq!(list.entries[0].revision, 1);
    assert_eq!(list.events[0].action, "added");
    s.set(PROJECT, ENV, "SERVICE_KEY", value(), 1).unwrap();
    let second = fs::read(f.file()).unwrap();
    assert_ne!(
        &first[MAGIC.len()..MAGIC.len() + 24],
        &second[MAGIC.len()..MAGIC.len() + 24]
    );
    drop(s);
    let s = f.store();
    assert_eq!(
        s.load(PROJECT, ENV).unwrap().entries[0].value.as_str(),
        VALUE
    );
    s.remove(PROJECT, ENV, "SERVICE_KEY", 2).unwrap();
    assert!(s.list(PROJECT, ENV).unwrap().entries.is_empty());
    assert_eq!(
        s.list(PROJECT, ENV).unwrap().events.last().unwrap().action,
        "removed"
    );
    s.set(PROJECT, ENV, "SERVICE_KEY", value(), 0).unwrap();
    assert_eq!(s.list(PROJECT, ENV).unwrap().entries[0].revision, 4);
}
#[test]
fn stale_replacements_and_deletes_never_erase_a_new_value() {
    let f = Fixture::new();
    let s = f.store();
    s.set(PROJECT, ENV, "SERVICE_KEY", value(), 0).unwrap();
    s.set(
        PROJECT,
        ENV,
        "SERVICE_KEY",
        Zeroizing::new("new fixture".into()),
        1,
    )
    .unwrap();
    let before = fs::read(f.file()).unwrap();
    assert_eq!(
        s.set(PROJECT, ENV, "SERVICE_KEY", value(), 1),
        Err(Error::Conflict)
    );
    assert_eq!(
        s.remove(PROJECT, ENV, "SERVICE_KEY", 1),
        Err(Error::Conflict)
    );
    assert_eq!(before, fs::read(f.file()).unwrap());
}
#[test]
fn ciphertext_cannot_move_between_projects_environments_or_owners() {
    let f = Fixture::new();
    let s = f.store();
    s.set(PROJECT, ENV, "SERVICE_KEY", value(), 0).unwrap();
    let bytes = fs::read(f.file()).unwrap();
    for (project, env) in [
        (ProjectId::from_bytes([9; 16]), ENV),
        (PROJECT, Environment::Preview),
    ] {
        let path = f.data.join(Store::filename(project, env));
        fs::write(&path, &bytes).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(s.list(project, env), Err(Error::Unavailable)));
    }
    drop(s);
    let s = Store::open(File::open(&f.data).unwrap(), &f.key, [1; 16], &[]).unwrap();
    assert!(matches!(s.list(PROJECT, ENV), Err(Error::Unavailable)));
}
#[test]
fn tampering_truncation_wrong_key_and_unsafe_files_fail_closed() {
    let f = Fixture::new();
    let s = f.store();
    s.set(PROJECT, ENV, "SERVICE_KEY", value(), 0).unwrap();
    let original = fs::read(f.file()).unwrap();
    for mut bytes in [
        vec![],
        original[..MAGIC.len() + 24].to_vec(),
        original.clone(),
    ] {
        if bytes.len() == original.len() {
            *bytes.last_mut().unwrap() ^= 1;
        }
        fs::write(f.file(), &bytes).unwrap();
        assert!(matches!(s.list(PROJECT, ENV), Err(Error::Unavailable)));
        assert_eq!(
            s.set(PROJECT, ENV, "OTHER", value(), 0),
            Err(Error::Unavailable)
        );
        assert_eq!(bytes, fs::read(f.file()).unwrap());
    }
    fs::write(f.file(), &original).unwrap();
    fs::set_permissions(f.file(), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(s.list(PROJECT, ENV), Err(Error::Unavailable)));
    fs::set_permissions(f.file(), fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(f.file()).unwrap();
    symlink(&f.key, f.file()).unwrap();
    assert!(matches!(s.list(PROJECT, ENV), Err(Error::Unavailable)));
    fs::remove_file(f.file()).unwrap();
    fs::hard_link(&f.key, f.file()).unwrap();
    assert!(matches!(s.list(PROJECT, ENV), Err(Error::Unavailable)));
    fs::remove_file(f.file()).unwrap();
    fs::write(f.file(), original).unwrap();
    fs::set_permissions(f.file(), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&f.key, [0xb6; 32]).unwrap();
    assert!(matches!(s.list(PROJECT, ENV), Err(Error::Unavailable)));
    drop(s);
    let s = f.store();
    assert!(matches!(s.list(PROJECT, ENV), Err(Error::Unavailable)));
}
#[test]
fn key_and_storage_rules_reject_overlap_symlinks_and_second_writers() {
    let f = Fixture::new();
    let s = f.store();
    assert!(Store::open(File::open(&f.data).unwrap(), &f.key, OWNER, &[]).is_err());
    drop(s);
    for mode in [0o644, 0o700, 0o4600] {
        fs::set_permissions(&f.key, fs::Permissions::from_mode(mode)).unwrap();
        assert!(Store::open(File::open(&f.data).unwrap(), &f.key, OWNER, &[]).is_err());
    }
    fs::set_permissions(&f.key, fs::Permissions::from_mode(0o600)).unwrap();
    let link = f.key.with_extension("link");
    symlink(&f.key, &link).unwrap();
    assert!(Store::open(File::open(&f.data).unwrap(), &link, OWNER, &[]).is_err());
    assert!(
        Store::open(
            File::open(&f.data).unwrap(),
            &f.key,
            OWNER,
            &[f.key.parent().unwrap()]
        )
        .is_err()
    );
    let nested = f.data.join("key");
    fs::copy(&f.key, &nested).unwrap();
    assert!(Store::open(File::open(&f.data).unwrap(), &nested, OWNER, &[]).is_err());
    fs::write(&f.key, [1; 31]).unwrap();
    assert!(Store::open(File::open(&f.data).unwrap(), &f.key, OWNER, &[]).is_err());
}
#[test]
fn limits_and_bounded_audit_keep_last_mutations_without_values() {
    let f = Fixture::new();
    let s = f.store();
    for name in ["", "a", "A-B", "1KEY", "../KEY", &"A".repeat(65)] {
        assert_eq!(s.set(PROJECT, ENV, name, value(), 0), Err(Error::Invalid));
    }
    for v in [String::new(), "x".repeat(MAX_VALUE + 1), "has\0nul".into()] {
        assert_eq!(
            s.set(PROJECT, ENV, "KEY", Zeroizing::new(v), 0),
            Err(Error::Invalid)
        );
    }
    for i in 0..MAX_ENTRIES {
        s.set(PROJECT, ENV, &format!("KEY_{i}"), value(), 0)
            .unwrap();
    }
    assert_eq!(
        s.set(PROJECT, ENV, "EXCESS", value(), 0),
        Err(Error::Capacity)
    );
    let mut revision = s
        .list(PROJECT, ENV)
        .unwrap()
        .entries
        .iter()
        .find(|e| e.name == "KEY_0")
        .unwrap()
        .revision;
    for _ in 0..110 {
        s.set(PROJECT, ENV, "KEY_0", value(), revision).unwrap();
        revision = s
            .list(PROJECT, ENV)
            .unwrap()
            .entries
            .iter()
            .find(|e| e.name == "KEY_0")
            .unwrap()
            .revision;
    }
    let list = s.list(PROJECT, ENV).unwrap();
    assert_eq!(list.events.len(), MAX_EVENTS);
    assert_eq!(list.events.last().unwrap().revision, revision);
}
#[test]
fn replaced_directory_path_does_not_redirect_retained_store() {
    let f = Fixture::new();
    let s = f.store();
    let retained = f.data.with_extension("retained");
    fs::rename(&f.data, &retained).unwrap();
    fs::create_dir(&f.data).unwrap();
    fs::set_permissions(&f.data, fs::Permissions::from_mode(0o700)).unwrap();
    s.set(PROJECT, ENV, "KEY", value(), 0).unwrap();
    assert!(retained.join(Store::filename(PROJECT, ENV)).exists());
    assert!(!f.file().exists());
    fs::set_permissions(retained, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        s.set(PROJECT, ENV, "OTHER", value(), 0),
        Err(Error::Unavailable)
    );
}

#[test]
fn post_publication_sync_failure_is_uncertain_and_reload_reads_authenticated_state() {
    let f = Fixture::new();
    let s = f.store();
    s.set(PROJECT, ENV, "KEY", value(), 0).unwrap();
    let mut state = s.load(PROJECT, ENV).unwrap();
    state.entries[0].value = Zeroizing::new("replacement fixture".into());
    state.entries[0].revision = 2;
    record(&mut state, "KEY", "replaced", 2, now().unwrap());
    assert_eq!(
        s.save_with_sync(PROJECT, ENV, &state, |_| Err(std::io::Error::other(
            "fixture sync failure"
        ))),
        Err(Error::Uncertain)
    );
    assert_eq!(s.list(PROJECT, ENV).unwrap().entries[0].revision, 2);
    assert_eq!(
        s.load(PROJECT, ENV).unwrap().entries[0].value.as_str(),
        "replacement fixture"
    );
    // Orphaned encrypted pending data is never mistaken for a committed record.
    let pending = f.data.join(".project-secrets-disposable.tmp");
    fs::write(&pending, b"encrypted incomplete fixture").unwrap();
    s.set(PROJECT, ENV, "KEY", value(), 2).unwrap();
    assert_eq!(s.list(PROJECT, ENV).unwrap().entries[0].revision, 3);
    assert_eq!(fs::read(pending).unwrap(), b"encrypted incomplete fixture");
}
