//! Fresh owner review of an exact complete pending policy decision. No startup
//! repair, deletion, policy replacement or reset, including for interrupted revocation.
use super::*;
use crate::library::record_store::Pending;

/// A non-cloneable, process-local recovery review, not an authentication token.
/// It cannot be constructed from serialized event bytes or used as a normal
/// publication review. The trusted adapter must authenticate a fresh owner action.
/// ```compile_fail
/// use devoption_builds::library::publication::RecoveryReview;
/// fn forge() -> RecoveryReview { RecoveryReview {} }
/// ```
pub struct RecoveryReview {
    review: Review,
    linked: bool,
    pending_inode: (u64, u64),
}
impl RecoveryReview {
    pub fn decision(&self) -> &Review {
        &self.review
    }
    pub fn final_link_present(&self) -> bool {
        self.linked
    }
}
impl Publications {
    /// All ordinary handles must be closed first. Inspection takes the same
    /// exclusive lock but returns no read grant, then releases it for human review.
    /// No partial/corrupt record or inference from an existing candidate is accepted.
    pub fn review_recovery(
        path: &str,
        owner: OwnerId,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
    ) -> Result<RecoveryReview, Error> {
        if actor != owner {
            return Err(Error::Denied);
        }
        let (store, pending) = RecordStore::<Event>::open_pending(path)?;
        prepare(owner, catalog, candidates, &store, pending)
    }
    /// Reopen, acquire the exclusive lock and freshly verify all observations,
    /// retaining that same lock through completion. Consumes the review even on
    /// failure; after uncertain completion use ordinary status if open succeeds.
    pub fn approve_recovery(
        path: &str,
        owner: OwnerId,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
        review: RecoveryReview,
    ) -> Result<(), Error> {
        Self::recover_at(
            path,
            owner,
            actor,
            catalog,
            candidates,
            review,
            Instant::now(),
        )
    }
    fn recover_at(
        path: &str,
        owner: OwnerId,
        actor: OwnerId,
        catalog: &Catalog,
        candidates: &PackageStore,
        review: RecoveryReview,
        now: Instant,
    ) -> Result<(), Error> {
        if actor != owner || review.review.owner != actor {
            return Err(Error::Denied);
        }
        if now
            .checked_duration_since(review.review.issued)
            .is_none_or(|age| age >= REVIEW_LIFETIME)
            || review.review.catalog != *catalog
        {
            return Err(Error::Changed);
        }
        let (store, pending) = RecordStore::<Event>::open_pending(path)?;
        let fresh = prepare(owner, catalog, candidates, &store, pending)?;
        if review.review.ledger != fresh.review.ledger
            || review.review.event != fresh.review.event
            || review.linked != fresh.linked
            || review.pending_inode != fresh.pending_inode
        {
            return Err(Error::Changed);
        }
        // No cooperating mutation can occur between validation and completion:
        // the inspected handle still owns the original exclusive lock.
        if store.complete_recovery(&fresh.review.event)? != StageOutcome::Stored {
            return Err(Error::Changed);
        }
        Ok(())
    }
}
fn prepare(
    owner: OwnerId,
    catalog: &Catalog,
    candidates: &PackageStore,
    store: &RecordStore<Event>,
    pending: Pending<Event>,
) -> Result<RecoveryReview, Error> {
    let state = state_from(pending.prefix, owner)?;
    let event = pending.record;
    if event.owner != owner
        || event.sequence != state.events.len() as u64
        || event.previous != state.head()
    {
        return Err(Error::Integrity);
    }
    check_source(catalog, event.source, event.repository)?;
    if normalize(event.change.clone(), catalog, event.source)? != event.change {
        return Err(Error::Changed);
    }
    let package = candidates
        .read(&event.coordinates)?
        .ok_or(Error::Unavailable)?;
    if Sha256::digest(&package.encode()) != event.candidate
        || package.source().project() != event.source.to_bytes()
        || package.source().repository() != event.repository.to_bytes()
    {
        return Err(Error::Integrity);
    }
    let mut versions = state.versions;
    apply(&mut versions, &event)?;
    Ok(RecoveryReview {
        review: Review {
            event,
            catalog: catalog.clone(),
            owner,
            issued: Instant::now(),
            ledger: store.binding()?,
            package,
        },
        linked: pending.linked,
        pending_inode: pending.inode,
    })
}

#[cfg(test)]
mod tests;
