use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
use tempfile::{TempDir, tempdir};

// Deliberately public fixture values; no real accounts or credentials.
const ADDRESS: &str = "member@example.test";
const PASSWORD: &str = "a long fixture password";
fn app(n: u8) -> AppId {
    AppId::parse(&format!("{n:032x}")).unwrap()
}
fn org(n: u8) -> OrganizationId {
    OrganizationId::parse(&format!("{n:032x}")).unwrap()
}
fn fixture() -> (TempDir, AppIdentityService) {
    let directory = tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let service =
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)).unwrap();
    (directory, service)
}
async fn provision(service: &AppIdentityService, role: AppRole) -> AccountId {
    service
        .provision(ADDRESS.into(), PASSWORD.into(), role)
        .await
        .unwrap()
}
async fn login(service: &AppIdentityService) -> Credentials {
    service
        .sign_in(ADDRESS.into(), PASSWORD.into())
        .await
        .unwrap()
        .unwrap()
}
fn read(service: &AppIdentityService, c: &Credentials, p: Permission) -> Option<Principal> {
    service.authorize_read(c.session_token(), p).unwrap()
}

#[tokio::test]
async fn native_accounts_persist_while_sessions_do_not() {
    let (directory, service) = fixture();
    let id = provision(&service, AppRole::Member).await;
    let c = login(&service).await;
    assert_eq!(
        read(&service, &c, Permission::AccessPortal)
            .unwrap()
            .account,
        id
    );
    assert!(read(&service, &c, Permission::AccessAdmin).is_none());
    assert!(
        service
            .sign_in(ADDRESS.into(), "incorrect password".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .sign_in("absent@example.test".into(), PASSWORD.into())
            .await
            .unwrap()
            .is_none()
    );
    let file = directory.path().join(format!("app-{}.json", app(1).0));
    let snapshot = fs::read_to_string(&file).unwrap();
    assert!(!snapshot.contains(PASSWORD));
    assert!(snapshot.contains("$argon2id$v=19$m=19456,t=2,p=1$"));
    assert!(!snapshot.contains(&hex(c.session_token())));
    assert_eq!(
        fs::metadata(file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(service);
    let service =
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)).unwrap();
    assert!(read(&service, &c, Permission::AccessPortal).is_none());
    let next = login(&service).await;
    assert_eq!(
        read(&service, &next, Permission::AccessPortal)
            .unwrap()
            .account,
        id
    );
    assert_ne!(c.token, next.token);
    assert_ne!(c.token, c.csrf);
    assert_ne!(next.csrf, c.csrf);
}

#[tokio::test]
async fn csrf_logout_and_current_authorization_are_enforced() {
    let (_directory, service) = fixture();
    let id = provision(&service, AppRole::Admin).await;
    let c = login(&service).await;
    assert!(read(&service, &c, Permission::AccessAdmin).is_some());
    assert!(
        service
            .authorize_mutation(&c.token, &[], Permission::AccessAdmin)
            .unwrap()
            .is_none()
    );
    for i in 0..32 {
        let mut bad = c.csrf;
        bad[i] ^= 1;
        assert!(
            service
                .authorize_mutation(&c.token, &bad, Permission::AccessAdmin)
                .unwrap()
                .is_none()
        );
        assert!(!service.logout(&c.token, &bad).unwrap());
    }
    assert!(
        service
            .authorize_mutation(&c.token, &c.csrf, Permission::AccessAdmin)
            .unwrap()
            .is_some()
    );
    let second = login(&service).await;
    assert!(!service.logout(&c.token, &second.csrf).unwrap());
    assert!(service.logout(&c.token, &c.csrf).unwrap());
    assert!(read(&service, &c, Permission::AccessAdmin).is_none());
    assert!(read(&service, &second, Permission::AccessAdmin).is_some());
    service
        .configure(id, AppRole::Member, true, vec![])
        .await
        .unwrap();
    assert!(read(&service, &second, Permission::AccessAdmin).is_none());
    assert!(read(&service, &second, Permission::AccessPortal).is_none());
    let third = login(&service).await;
    assert!(read(&service, &third, Permission::AccessPortal).is_some());
    assert!(read(&service, &third, Permission::AccessAdmin).is_none());
}

#[tokio::test]
async fn disable_and_password_update_revoke_all_sessions_and_survive_reopen() {
    let (directory, service) = fixture();
    let id = provision(&service, AppRole::Member).await;
    let before = login(&service).await;
    service
        .configure(id.clone(), AppRole::Member, false, vec![])
        .await
        .unwrap();
    assert!(read(&service, &before, Permission::AccessPortal).is_none());
    assert!(
        service
            .sign_in(ADDRESS.into(), PASSWORD.into())
            .await
            .unwrap()
            .is_none()
    );
    service
        .configure(id.clone(), AppRole::Member, true, vec![])
        .await
        .unwrap();
    assert!(read(&service, &before, Permission::AccessPortal).is_none());
    let current = login(&service).await;
    let replacement = "a replacement fixture password";
    service
        .replace_password(id.clone(), replacement.into())
        .await
        .unwrap();
    assert!(read(&service, &current, Permission::AccessPortal).is_none());
    assert!(
        service
            .sign_in(ADDRESS.into(), PASSWORD.into())
            .await
            .unwrap()
            .is_none()
    );
    drop(service);
    let service =
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)).unwrap();
    let c = service
        .sign_in(ADDRESS.into(), replacement.into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        read(&service, &c, Permission::AccessPortal)
            .unwrap()
            .account,
        id
    );
}

#[tokio::test]
async fn organization_admins_never_gain_application_admin_or_another_organizations_access() {
    let (_directory, service) = fixture();
    let id = provision(&service, AppRole::Member).await;
    service
        .configure(
            id.clone(),
            AppRole::Member,
            true,
            vec![Membership {
                organization: org(2),
                role: OrganizationRole::Admin,
            }],
        )
        .await
        .unwrap();
    let c = login(&service).await;
    assert!(read(&service, &c, Permission::ManageOrganization(org(2))).is_some());
    assert!(read(&service, &c, Permission::ReadOrganization(org(2))).is_some());
    assert!(read(&service, &c, Permission::ReadOrganization(org(3))).is_none());
    assert!(read(&service, &c, Permission::ManageOrganization(org(3))).is_none());
    assert!(read(&service, &c, Permission::AccessAdmin).is_none());
    service
        .configure(
            id.clone(),
            AppRole::Member,
            true,
            vec![Membership {
                organization: org(2),
                role: OrganizationRole::Member,
            }],
        )
        .await
        .unwrap();
    let next = login(&service).await;
    assert!(read(&service, &c, Permission::ReadOrganization(org(2))).is_none());
    assert!(read(&service, &next, Permission::ReadOrganization(org(2))).is_some());
    assert!(read(&service, &next, Permission::ManageOrganization(org(2))).is_none());
    service
        .configure(id, AppRole::Admin, true, vec![])
        .await
        .unwrap();
    let admin = login(&service).await;
    assert!(read(&service, &admin, Permission::AccessAdmin).is_some());
    assert!(read(&service, &admin, Permission::ReadOrganization(org(2))).is_none());
}

#[tokio::test]
async fn namespaces_clones_and_the_exclusive_store_lock_are_distinct() {
    let (directory, service) = fixture();
    let id = provision(&service, AppRole::Admin).await;
    let c = login(&service).await;
    assert!(matches!(
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)),
        Err(Error::Busy)
    ));
    let other =
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(2)).unwrap();
    assert!(read(&other, &c, Permission::AccessAdmin).is_none());
    assert!(
        other
            .sign_in(ADDRESS.into(), PASSWORD.into())
            .await
            .unwrap()
            .is_none()
    );
    let other_id = provision(&other, AppRole::Member).await;
    assert_ne!(id, other_id);
    let theirs = login(&other).await;
    assert!(read(&service, &theirs, Permission::AccessPortal).is_none());
    let clone = service.clone();
    clone
        .configure(id, AppRole::Member, true, vec![])
        .await
        .unwrap();
    assert!(read(&service, &c, Permission::AccessPortal).is_none());
    assert!(!directory.path().join("owner.bin").exists());
}

#[tokio::test]
async fn absolute_session_expiry_and_capacity_do_not_slide_or_evict() {
    let (_directory, service) = fixture();
    provision(&service, AppRole::Member).await;
    let c = login(&service).await;
    {
        let mut sessions = service.state.sessions.lock().unwrap();
        sessions[0].expires = Instant::now();
    }
    assert!(read(&service, &c, Permission::AccessPortal).is_none());
    assert!(!service.logout(&c.token, &c.csrf).unwrap());
    let current = login(&service).await;
    let deadline = service.state.sessions.lock().unwrap()[0].expires;
    assert!(read(&service, &current, Permission::AccessPortal).is_some());
    assert_eq!(service.state.sessions.lock().unwrap()[0].expires, deadline);
    {
        let mut sessions = service.state.sessions.lock().unwrap();
        for i in 1..MAX_SESSIONS {
            sessions.push(Session {
                account: AccountId(format!("{i:032x}")),
                revision: 1,
                token: [i as u8; 32],
                csrf: [0; 32],
                expires: deadline,
            });
        }
    }
    assert!(matches!(
        service.sign_in(ADDRESS.into(), PASSWORD.into()).await,
        Err(Error::Capacity)
    ));
    assert!(read(&service, &current, Permission::AccessPortal).is_some());
}

#[tokio::test]
async fn sign_in_limits_are_shared_and_busy_work_is_bounded() {
    let (_directory, service) = fixture();
    provision(&service, AppRole::Member).await;
    // The permit remains owned by the actual blocking worker, even if its
    // awaiting request is cancelled. Here hold that worker deterministically.
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let clone = service.clone();
    let task = tokio::spawn(async move {
        clone
            .work(move |_| {
                started_tx.send(()).unwrap();
                finish_rx.recv().unwrap();
                Ok(())
            })
            .await
    });
    started_rx.await.unwrap();
    task.abort();
    assert!(matches!(
        service.sign_in(ADDRESS.into(), PASSWORD.into()).await,
        Err(Error::Busy)
    ));
    finish_tx.send(()).unwrap();
    let _permit = service.state.worker.acquire().await.unwrap();
    for _ in 1..MAX_ATTEMPTS {
        assert!(matches!(
            service.sign_in(ADDRESS.into(), PASSWORD.into()).await,
            Err(Error::Busy)
        ));
    }
    assert!(matches!(
        service
            .clone()
            .sign_in(ADDRESS.into(), PASSWORD.into())
            .await,
        Err(Error::RateLimited)
    ));
    {
        let mut attempts = service.state.attempts.lock().unwrap();
        for t in attempts.iter_mut() {
            *t = Instant::now() - ATTEMPT_WINDOW;
        }
    }
    drop(_permit);
    assert!(read(&service, &login(&service).await, Permission::AccessPortal).is_some());
}

#[tokio::test]
async fn input_policy_rejects_unsafe_identifiers_duplicates_and_short_passwords() {
    let (_directory, service) = fixture();
    for address in [
        "",
        "bad",
        "a@@example.test",
        "a@-example.test",
        "a\n@example.test",
        "a@localhost",
        "a@e..test",
    ] {
        assert_eq!(
            service
                .provision(address.into(), PASSWORD.into(), AppRole::Member)
                .await,
            Err(Error::Invalid)
        );
    }
    for secret in ["", "short", &"x".repeat(1025)] {
        assert_eq!(
            service
                .provision(ADDRESS.into(), secret.into(), AppRole::Member)
                .await,
            Err(Error::Invalid)
        );
    }
    let id = service
        .provision(
            " MEMBER@EXAMPLE.TEST ".into(),
            PASSWORD.into(),
            AppRole::Member,
        )
        .await
        .unwrap();
    assert_eq!(
        service
            .provision(ADDRESS.into(), PASSWORD.into(), AppRole::Admin)
            .await,
        Err(Error::Exists)
    );
    let memberships = vec![
        Membership {
            organization: org(2),
            role: OrganizationRole::Admin
        };
        2
    ];
    assert_eq!(
        service
            .configure(id, AppRole::Admin, true, memberships)
            .await,
        Err(Error::Invalid)
    );
    assert!(read(&service, &login(&service).await, Permission::AccessAdmin).is_none());
    for id in ["", "../owner", "0", "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"] {
        assert_eq!(AppId::parse(id), Err(Error::Invalid));
    }
}

#[tokio::test]
async fn storage_refuses_unsafe_paths_corruption_and_cross_namespace_records() {
    let (directory, service) = fixture();
    provision(&service, AppRole::Member).await;
    let c = login(&service).await;
    let record = directory.path().join(format!("app-{}.json", app(1).0));
    let snapshot = fs::read(&record).unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        service.authorize_read(&c.token, Permission::AccessPortal),
        Err(Error::Unavailable)
    ));
    drop(service);
    assert!(matches!(
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)),
        Err(Error::Unavailable)
    ));
    fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
    for bytes in [b"{}".as_slice(), b"[2,1,[]]", b"not json"] {
        fs::write(&record, bytes).unwrap();
        assert!(matches!(
            AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)),
            Err(Error::Unavailable)
        ));
    }
    fs::write(&record, &snapshot).unwrap();
    let other = directory.path().join(format!("app-{}.json", app(2).0));
    fs::write(&other, snapshot).unwrap();
    fs::set_permissions(&other, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(2)),
        Err(Error::Unavailable)
    ));
    fs::remove_file(&record).unwrap();
    symlink(&other, &record).unwrap();
    assert!(matches!(
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)),
        Err(Error::Unavailable)
    ));
    let link = directory.path().join("directory-link");
    symlink(directory.path(), &link).unwrap();
    assert!(matches!(
        AppIdentityService::open(&link, app(1)),
        Err(Error::Unavailable)
    ));
}

#[tokio::test]
async fn account_bounds_and_invalid_snapshots_fail_closed() {
    let (directory, service) = fixture();
    provision(&service, AppRole::Member).await;
    {
        let mut data = service.state.store.data().unwrap();
        let account = data.accounts[0].clone();
        data.accounts.resize(MAX_ACCOUNTS, account);
    }
    assert_eq!(
        service
            .provision(
                "other@example.test".into(),
                PASSWORD.into(),
                AppRole::Member
            )
            .await,
        Err(Error::Capacity)
    );
    drop(service);
    let record = directory.path().join(format!("app-{}.json", app(1).0));
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    let mut bad = vec![];
    let mut value = original.clone();
    value[2][0][3] = serde_json::json!("super-admin");
    bad.push(value);
    let mut value = original.clone();
    value[2][0][2] = serde_json::json!("plaintext");
    bad.push(value);
    let mut value = original.clone();
    value[2][0][5] = serde_json::json!(0);
    bad.push(value);
    let mut value = original.clone();
    value[2]
        .as_array_mut()
        .unwrap()
        .push(original[2][0].clone());
    bad.push(value);
    let mut value = original.clone();
    value[2][0]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(true));
    bad.push(value);
    for value in bad {
        fs::write(&record, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(matches!(
            AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)),
            Err(Error::Unavailable)
        ));
    }
}

#[tokio::test]
async fn write_failure_latches_unavailable_and_never_authorizes_stale_roles() {
    let (directory, service) = fixture();
    provision(&service, AppRole::Admin).await;
    let c = login(&service).await;
    let record = directory.path().join(format!("app-{}.json", app(1).0));
    let original = fs::read(&record).unwrap();
    {
        let mut data = service.state.store.data().unwrap();
        assert!(matches!(
            service.authorize_read(&c.token, Permission::AccessAdmin),
            Err(Error::Busy)
        ));
        let mut next = data.accounts.clone();
        next[0].enabled = false;
        // Deterministically fail publication after creating/writing the pending
        // file, without relying on scheduling, privileged chmod or disk filling.
        fs::remove_file(&record).unwrap();
        fs::create_dir(&record).unwrap();
        assert_eq!(
            service.state.store.commit(&mut data, next),
            Err(Error::Unavailable)
        );
    }
    assert!(matches!(
        service.authorize_read(&c.token, Permission::AccessAdmin),
        Err(Error::Unavailable)
    ));
    assert!(matches!(
        service.sign_in(ADDRESS.into(), PASSWORD.into()).await,
        Err(Error::Unavailable)
    ));
    // Even restoring the last valid snapshot does not unlatch this service.
    fs::remove_dir(&record).unwrap();
    fs::write(&record, original).unwrap();
    fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        service.authorize_read(&c.token, Permission::AccessAdmin),
        Err(Error::Unavailable)
    ));
    assert!(
        !directory.path().read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
    drop(service);
    let reopened =
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)).unwrap();
    assert!(read(&reopened, &c, Permission::AccessAdmin).is_none());
    assert!(read(&reopened, &login(&reopened).await, Permission::AccessAdmin).is_some());
}

#[tokio::test]
async fn equal_passwords_get_distinct_salts_and_hardlinked_snapshots_are_refused() {
    let (directory, service) = fixture();
    provision(&service, AppRole::Member).await;
    service
        .provision(
            "second@example.test".into(),
            PASSWORD.into(),
            AppRole::Member,
        )
        .await
        .unwrap();
    {
        let data = service.state.store.data().unwrap();
        assert!(data.accounts[0].hash != data.accounts[1].hash);
    }
    let c = login(&service).await;
    let record = directory.path().join(format!("app-{}.json", app(1).0));
    fs::hard_link(&record, directory.path().join("unexpected-link")).unwrap();
    assert!(matches!(
        service.authorize_read(&c.token, Permission::AccessPortal),
        Err(Error::Unavailable)
    ));
    drop(service);
    assert!(matches!(
        AppIdentityService::open(&directory.path().canonicalize().unwrap(), app(1)),
        Err(Error::Unavailable)
    ));
}
