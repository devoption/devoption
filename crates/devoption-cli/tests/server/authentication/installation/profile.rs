use super::super::profile::{check, read, save_request};
use super::*;

#[test]
fn documented_backup_restores_profile_with_the_same_owner_and_rejects_missing_metadata() {
    let (_owner, data) = configured();
    let (server, address) = start_with(&data);
    let session = sign_in(address);
    check(
        &request(address, &save_request(address, &session, "Fixture Person")),
        204,
    );
    drop(server);
    let original = fs::read(data.join("profile.bin")).unwrap();
    let (_temporary, root) = data_directory::fixture();
    let backup = root.join("backup");
    fs::DirBuilder::new().mode(0o700).create(&backup).unwrap();
    assert!(guide_copy("data-backup", &data, &backup).status.success());
    assert_eq!(
        fs::read(backup.join("profile-state")).unwrap(),
        b"present\n"
    );
    let restored = root.join("restored");
    fs::DirBuilder::new().mode(0o700).create(&restored).unwrap();
    assert!(
        guide_copy("data-restore", &backup, &restored)
            .status
            .success()
    );
    assert_eq!(fs::read(restored.join("profile.bin")).unwrap(), original);
    let (server, address) = start_with(&restored);
    assert_eq!(
        check(
            &read(address, "GET", "/account/profile", Some(&sign_in(address))),
            200
        ),
        "Fixture Person"
    );
    drop(server);
    for (index, marker) in [None, Some("unknown\n"), Some("absent\n")]
        .into_iter()
        .enumerate()
    {
        if let Some(marker) = marker {
            fs::write(backup.join("profile-state"), marker).unwrap();
        } else {
            fs::remove_file(backup.join("profile-state")).unwrap();
        }
        let target = root.join(format!("rejected-{index}"));
        fs::DirBuilder::new().mode(0o700).create(&target).unwrap();
        assert!(
            !guide_copy("data-restore", &backup, &target)
                .status
                .success()
        );
        assert!(!target.join("owner.bin").exists());
    }
    assert_eq!(fs::read(data.join("profile.bin")).unwrap(), original);
    assert_eq!(fs::read(backup.join("profile.bin")).unwrap(), original);
}
