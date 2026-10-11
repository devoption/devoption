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
const SECOND_ID: IdentityId = IdentityId::from_bytes([2; 16]);

fn session(identity: IdentityId, token: [u8; TOKEN_BYTES], now: Instant) -> Session {
    Session::new_at(identity, token, MAX_LIFETIME, now).unwrap()
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
    slot.activate(session(FIRST_ID, FIRST, Instant::now()), &FIRST)
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
}
