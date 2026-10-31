use super::*;
use crate::session::{MAX_LIFETIME, TOKEN_BYTES};
use std::{
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

const FIRST: [u8; TOKEN_BYTES] = [0x11; TOKEN_BYTES];
const SECOND: [u8; TOKEN_BYTES] = [0x22; TOKEN_BYTES];
const FIRST_ID: IdentityId = IdentityId::from_bytes([1; 16]);
const SECOND_ID: IdentityId = FIRST_ID; // Rotation tests use the same account.
const FIRST_CSRF: [u8; TOKEN_BYTES] = [0x33; TOKEN_BYTES];
const SECOND_CSRF: [u8; TOKEN_BYTES] = [0x44; TOKEN_BYTES];

fn session(identity: IdentityId, token: [u8; TOKEN_BYTES], now: Instant) -> Session {
    Session::new_at(identity, token, MAX_LIFETIME, now).unwrap()
}

#[test]
fn csrf_recovery_never_renews_or_revives_and_observes_rotation_and_logout() {
    let slot = SessionSlot::default();
    let now = Instant::now();
    assert_eq!(slot.recover_browser_csrf(&FIRST), None);
    slot.activate(session(FIRST_ID, FIRST, now), &FIRST)
        .unwrap();
    assert_eq!(slot.recover_browser_csrf(&FIRST), None); // bearer-only fixture
    slot.activate(session(FIRST_ID, FIRST, now).with_csrf(FIRST_CSRF), &FIRST)
        .unwrap();
    assert_eq!(slot.recover_browser_csrf(&SECOND), None);
    assert_eq!(
        slot.recover_csrf_with_clock(&FIRST, || {
            assert!(slot.active.try_lock().is_err());
            now + MAX_LIFETIME - Duration::from_nanos(1)
        }),
        Some(FIRST_CSRF)
    );
    assert_eq!(
        slot.recover_csrf_with_clock(&FIRST, || now + MAX_LIFETIME),
        None
    );
    assert_eq!(slot.recover_csrf_with_clock(&FIRST, || now), None);
    slot.activate(
        session(SECOND_ID, SECOND, now).with_csrf(SECOND_CSRF),
        &SECOND,
    )
    .unwrap();
    assert_eq!(slot.recover_browser_csrf(&FIRST), None);
    assert_eq!(slot.recover_browser_csrf(&SECOND), Some(SECOND_CSRF));
    assert!(slot.logout_browser(Some(&SECOND), Some(&SECOND_CSRF)));
    assert_eq!(slot.recover_browser_csrf(&SECOND), None);
    let _ = std::panic::catch_unwind(|| {
        let _lock = slot.active.lock().unwrap();
        panic!("fixture poison");
    });
    assert_eq!(slot.recover_browser_csrf(&SECOND), None);
}

#[test]
fn browser_rotation_and_logout_require_the_matching_pair_in_shared_state() {
    let slot = Arc::new(SessionSlot::default());
    let shared = Arc::clone(&slot);
    let other = SessionSlot::default();
    let now = Instant::now();
    assert_eq!(
        slot.authenticate_browser(Some(&FIRST), Some(&FIRST_CSRF)),
        None
    );
    slot.activate(session(FIRST_ID, FIRST, now).with_csrf(FIRST_CSRF), &FIRST)
        .unwrap();
    assert_eq!(
        shared.authenticate_browser(Some(&FIRST), Some(&FIRST_CSRF)),
        Some(FIRST_ID)
    );
    assert_eq!(
        other.authenticate_browser(Some(&FIRST), Some(&FIRST_CSRF)),
        None
    );
    slot.activate(
        session(SECOND_ID, SECOND, now).with_csrf(SECOND_CSRF),
        &SECOND,
    )
    .unwrap();
    for (token, csrf) in [
        (Some(&FIRST[..]), Some(&FIRST_CSRF[..])),
        (Some(&FIRST[..]), Some(&SECOND_CSRF[..])),
        (Some(&SECOND[..]), Some(&FIRST_CSRF[..])),
        (Some(&SECOND[..]), None),
        (None, Some(&SECOND_CSRF[..])),
        (Some(&SECOND[..]), Some(&SECOND_CSRF[..31])),
    ] {
        assert_eq!(shared.authenticate_browser(token, csrf), None);
        assert!(!shared.logout_browser(token, csrf));
        assert_eq!(
            slot.authenticate_browser(Some(&SECOND), Some(&SECOND_CSRF)),
            Some(SECOND_ID)
        );
    }
    assert!(shared.logout_browser(Some(&SECOND), Some(&SECOND_CSRF)));
    assert_eq!(
        slot.authenticate_browser(Some(&SECOND), Some(&SECOND_CSRF)),
        None
    );
    assert_eq!(slot.authenticate(Some(&SECOND)), None);
    assert!(!slot.logout_browser(Some(&SECOND), Some(&SECOND_CSRF)));
}

#[test]
fn browser_validation_observes_the_original_deadline_after_locking() {
    let slot = SessionSlot::default();
    let now = Instant::now();
    let deadline = now + MAX_LIFETIME;
    let activation = deadline - Duration::from_secs(10);
    slot.activate_with_clock(
        session(FIRST_ID, FIRST, now).with_csrf(FIRST_CSRF),
        &FIRST,
        || activation,
    )
    .unwrap();
    assert_eq!(
        slot.browser_with_clock(Some(&FIRST), Some(&FIRST_CSRF), false, || {
            assert!(
                slot.active.try_lock().is_err(),
                "clock must be observed while holding the session lock"
            );
            deadline - Duration::from_nanos(1)
        }),
        Some(FIRST_ID)
    );
    assert_eq!(
        slot.browser_with_clock(Some(&FIRST), Some(&FIRST_CSRF), true, || deadline),
        None
    );
    assert_eq!(
        slot.browser_with_clock(Some(&FIRST), Some(&FIRST_CSRF), false, || activation),
        None
    );
}

#[test]
fn racing_browser_logout_cannot_remove_a_replacement_pair() {
    let slot = Arc::new(SessionSlot::default());
    slot.activate(
        session(FIRST_ID, FIRST, Instant::now()).with_csrf(FIRST_CSRF),
        &FIRST,
    )
    .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let replacement_slot = Arc::clone(&slot);
    let replacement_barrier = Arc::clone(&barrier);
    let replacement = thread::spawn(move || {
        replacement_barrier.wait();
        replacement_slot
            .activate(
                session(SECOND_ID, SECOND, Instant::now()).with_csrf(SECOND_CSRF),
                &SECOND,
            )
            .unwrap();
    });
    let logout_slot = Arc::clone(&slot);
    let logout = thread::spawn(move || {
        barrier.wait();
        logout_slot.logout_browser(Some(&FIRST), Some(&FIRST_CSRF))
    });
    replacement.join().unwrap();
    logout.join().unwrap();
    assert_eq!(
        slot.authenticate_browser(Some(&SECOND), Some(&SECOND_CSRF)),
        Some(SECOND_ID)
    );
    assert_eq!(
        slot.authenticate_browser(Some(&SECOND), Some(&FIRST_CSRF)),
        None
    );
}

#[test]
fn empty_slots_deny_access_and_only_shared_state_observes_activation() {
    let slot = Arc::new(SessionSlot::default());
    let other = SessionSlot::default();
    assert_eq!(slot.authenticate(Some(&FIRST)), None);
    assert!(!slot.logout(None));
    slot.activate(session(FIRST_ID, FIRST, Instant::now()), &FIRST)
        .unwrap();
    let clone = Arc::clone(&slot);
    assert_eq!(
        thread::spawn(move || clone.authenticate(Some(&FIRST)))
            .join()
            .unwrap(),
        Some(FIRST_ID)
    );
    assert_eq!(other.authenticate(Some(&FIRST)), None);
}

#[test]
fn invalid_activation_preserves_the_current_session() {
    let slot = SessionSlot::default();
    let now = Instant::now() + MAX_LIFETIME;
    slot.activate_with_clock(session(FIRST_ID, FIRST, now), &FIRST, || now)
        .unwrap();
    let mut revoked = session(SECOND_ID, SECOND, now);
    revoked.revoke();
    for (candidate, token) in [
        (revoked, &SECOND[..]),
        (session(SECOND_ID, SECOND, now - MAX_LIFETIME), &SECOND[..]),
        (session(SECOND_ID, SECOND, now), &FIRST[..]),
        (session(SECOND_ID, SECOND, now), &SECOND[..31]),
    ] {
        assert_eq!(
            slot.activate_with_clock(candidate, token, || now),
            Err(ActivationError::InvalidSession)
        );
        assert_eq!(
            slot.authenticate_with_clock(Some(&FIRST), || now),
            Some(FIRST_ID)
        );
        assert_eq!(slot.authenticate_with_clock(Some(&SECOND), || now), None);
    }
}

#[test]
fn rotation_rejects_the_old_token_and_logout_requires_the_current_one() {
    let slot = SessionSlot::default();
    let now = Instant::now();
    slot.activate_with_clock(session(FIRST_ID, FIRST, now), &FIRST, || now)
        .unwrap();
    slot.activate_with_clock(session(SECOND_ID, SECOND, now), &SECOND, || now)
        .unwrap();
    assert_eq!(slot.authenticate_with_clock(Some(&FIRST), || now), None);
    for rejected in [None, Some(&FIRST[..]), Some(&SECOND[..31]), Some(&[][..])] {
        assert!(!slot.logout_with_clock(rejected, || now));
        assert_eq!(
            slot.authenticate_with_clock(Some(&SECOND), || now),
            Some(SECOND_ID)
        );
    }
    assert!(slot.logout_with_clock(Some(&SECOND), || now));
    assert_eq!(slot.authenticate_with_clock(Some(&SECOND), || now), None);
    assert!(!slot.logout_with_clock(Some(&SECOND), || now));
}

#[test]
fn activation_preserves_the_original_deadline_and_expired_logout_is_rejected() {
    let slot = SessionSlot::default();
    let now = Instant::now();
    let deadline = now + MAX_LIFETIME;
    let activation = deadline - Duration::from_secs(10);
    slot.activate_with_clock(session(FIRST_ID, FIRST, now), &FIRST, || activation)
        .unwrap();
    assert_eq!(
        slot.authenticate_with_clock(Some(&FIRST), || deadline - Duration::from_nanos(1)),
        Some(FIRST_ID)
    );
    assert!(!slot.logout_with_clock(Some(&FIRST), || deadline));
    assert_eq!(
        slot.authenticate_with_clock(Some(&FIRST), || deadline),
        None
    );
    // An observed expiry stays revoked, even with an earlier test-clock value.
    assert_eq!(
        slot.authenticate_with_clock(Some(&FIRST), || activation),
        None
    );
}

#[test]
fn racing_stale_logout_cannot_remove_the_replacement() {
    let slot = Arc::new(SessionSlot::default());
    slot.activate(session(FIRST_ID, FIRST, Instant::now()), &FIRST)
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let replacement_slot = Arc::clone(&slot);
    let replacement_barrier = Arc::clone(&barrier);
    let replacement = thread::spawn(move || {
        replacement_barrier.wait();
        replacement_slot
            .activate(session(SECOND_ID, SECOND, Instant::now()), &SECOND)
            .unwrap();
    });
    let logout_slot = Arc::clone(&slot);
    let logout = thread::spawn(move || {
        barrier.wait();
        logout_slot.logout(Some(&FIRST))
    });
    replacement.join().unwrap();
    logout.join().unwrap();
    assert_eq!(slot.authenticate(Some(&FIRST)), None);
    assert_eq!(slot.authenticate(Some(&SECOND)), Some(SECOND_ID));
}

#[test]
fn poisoned_state_rejects_activation_authentication_and_logout() {
    let slot = Arc::new(SessionSlot::default());
    slot.activate(
        session(FIRST_ID, FIRST, Instant::now()).with_csrf(FIRST_CSRF),
        &FIRST,
    )
    .unwrap();
    let clone = Arc::clone(&slot);
    assert!(
        thread::spawn(move || {
            let _guard = clone.active.lock().unwrap();
            panic!("fixture session state failure");
        })
        .join()
        .is_err()
    );
    assert_eq!(
        slot.activate(session(SECOND_ID, SECOND, Instant::now()), &SECOND),
        Err(ActivationError::Unavailable)
    );
    assert_eq!(slot.authenticate(Some(&FIRST)), None);
    assert_eq!(slot.authenticate(Some(&SECOND)), None);
    assert!(!slot.logout(Some(&FIRST)));
    assert_eq!(
        slot.authenticate_browser(Some(&FIRST), Some(&FIRST_CSRF)),
        None
    );
    assert!(!slot.logout_browser(Some(&FIRST), Some(&FIRST_CSRF)));
}

#[test]
fn different_accounts_keep_independent_sessions_and_csrf_pairs() {
    let slot = SessionSlot::default();
    let other = IdentityId::from_bytes([2; 16]);
    let now = Instant::now();
    slot.activate(session(FIRST_ID, FIRST, now).with_csrf(FIRST_CSRF), &FIRST)
        .unwrap();
    slot.activate(session(other, SECOND, now).with_csrf(SECOND_CSRF), &SECOND)
        .unwrap();
    assert_eq!(
        slot.authenticate_browser(Some(&FIRST), Some(&SECOND_CSRF)),
        None
    );
    assert_eq!(
        slot.authenticate_browser(Some(&FIRST), Some(&FIRST_CSRF)),
        Some(FIRST_ID)
    );
    assert!(slot.logout_browser(Some(&SECOND), Some(&SECOND_CSRF)));
    assert_eq!(slot.authenticate(Some(&FIRST)), Some(FIRST_ID));
    slot.revoke_identity(FIRST_ID);
    assert_eq!(slot.authenticate(Some(&FIRST)), None);
}
