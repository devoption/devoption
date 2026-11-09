use super::super::tests::{Fixture, private};
use super::*;

#[test]
fn staging_routes_preserve_owner_session_csrf_and_disabled_boundary() {
    let f = Fixture::new();
    let path = format!(
        "/projects/{}/builds/attempts/{}/registry/prepare",
        hex(&f.source.to_bytes()),
        "00".repeat(16)
    );
    let request = || f.request("POST", &path, "", false);
    assert_eq!(f.send_request(request()).0, StatusCode::SERVICE_UNAVAILABLE);
    let mut anonymous = request();
    anonymous.headers_mut().remove("cookie");
    assert_eq!(f.send_request(anonymous).0, StatusCode::UNAUTHORIZED);
    let mut csrf = request();
    csrf.headers_mut()
        .remove(crate::browser_session::CSRF_HEADER);
    assert_eq!(f.send_request(csrf).0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        f.send_request(f.request("POST", &path, "", true)).0,
        StatusCode::FORBIDDEN
    );
    let mut origin = request();
    origin
        .headers_mut()
        .insert("origin", "http://other.example.test".parse().unwrap());
    assert_eq!(f.send_request(origin).0, StatusCode::FORBIDDEN);
    assert!(
        f.projects
            .registry
            .as_ref()
            .unwrap()
            .blocking_lock()
            .pending
            .is_none()
    );
}

#[test]
fn stage_reviews_are_single_use_bound_to_attempt_project_action_and_expiry() {
    let f = Fixture::new();
    let history_path = f.registry.parent().unwrap().join("history");
    private(&history_path);
    let history = History::open(history_path.to_str().unwrap()).unwrap();
    let catalog = f.projects.directory.load().unwrap().unwrap();
    let selected = Selection {
        catalog: &catalog,
        root: f.projects.root.as_ref().unwrap(),
        history: &history,
        work: &history_path,
        project: f.source,
        attempt: JobId::from_bytes([1; 16]),
    };
    let mut engine = f.projects.registry.as_ref().unwrap().blocking_lock();
    let pending = || Pending {
        token: "ab".repeat(16),
        expires: Instant::now() + Duration::from_secs(300),
        project: Some(f.source),
        coordinates: Some(f.package.source().coordinates().clone()),
        // Only exercise refusal before artifact acquisition; this plan-only fixture
        // cannot produce successful-build evidence or enter the successful path.
        decision: Decision::Ingestion(Box::new(Review {
            candidate: f.package.clone(),
            catalog: catalog.clone(),
            attempt: selected.attempt,
        })),
    };
    for change in 0..5 {
        let mut p = pending();
        match change {
            0 => p.token = "wrong".into(),
            1 => p.project = Some(f.consumer),
            2 => p.expires = Instant::now() - Duration::from_secs(1),
            3 => {
                let Decision::Ingestion(r) = &mut p.decision else {
                    unreachable!()
                };
                r.attempt = JobId::from_bytes([2; 16]);
            }
            _ => {
                let Decision::Ingestion(r) = &mut p.decision else {
                    unreachable!()
                };
                r.catalog = Catalog::default();
            }
        }
        engine.pending = Some(p);
        assert_eq!(
            selected
                .operate(&mut engine, true, &"ab".repeat(16), || true)
                .unwrap_err(),
            StatusCode::CONFLICT
        );
        assert!(engine.pending.is_none());
        assert_eq!(
            selected
                .operate(&mut engine, true, &"ab".repeat(16), || true)
                .unwrap_err(),
            StatusCode::CONFLICT
        );
    }
    engine.pending = Some(pending());
    // The publication endpoint also rejects and consumes staging tokens.
    assert_eq!(
        super::super::operate(
            &mut engine,
            &catalog,
            Some((
                hex(&f.source.to_bytes()),
                "shared-tool".into(),
                "0.1.0".into()
            )),
            None,
            Action::Approve,
            &"ab".repeat(16)
        )
        .unwrap_err(),
        StatusCode::CONFLICT
    );
    assert!(engine.pending.is_none());
    engine.pending = Some(pending());
    assert_eq!(
        selected
            .operate(&mut engine, false, "nonempty", || true)
            .unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    assert!(engine.pending.is_none());
    assert_eq!(
        selected
            .operate(&mut engine, false, "", || true)
            .unwrap_err(),
        StatusCode::NOT_FOUND
    );
    engine.policy = None;
    assert_eq!(
        selected
            .operate(&mut engine, false, "", || true)
            .unwrap_err(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(history.load().unwrap().is_empty());
}
