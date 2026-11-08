use super::*;

#[test]
fn admission_payload_and_policy_are_bounded() {
    assert!(payload("[]").is_err());
    assert!(payload(r#"{"a":{"b":1,"b":2}}"#).is_err());
    assert!(payload(&" ".repeat(24577)).is_err());
    assert!(payload(r#"{"event":"fixture"}"#).is_ok());
    let mut p:Policy=serde_json::from_value(serde_json::json!({"project":"a".repeat(32),"environment":"preview","secret":"SERVICE_API_KEY","revision":1,"endpoint":"https://api.example.test/events","receiver_dedup_seconds":172800})).unwrap();
    assert!(p.valid(&"a".repeat(32)));
    assert!(!p.valid(&"b".repeat(32)));
    let original = p.digest();
    p.revision += 1;
    assert_ne!(original, p.digest());
    p.receiver_dedup_seconds = 10;
    assert!(!p.valid(&"a".repeat(32)));
}

#[test]
#[ignore = "explicit disposable PostgreSQL/TLS worker process proof; no production grants"]
fn manual_delivery_worker() {
    let path =
        |name| std::path::PathBuf::from(std::env::var_os(name).expect("explicit fixture path"));
    let grant = path("DEVOPTION_JOB_PROOF_GRANT");
    let credential = path("DEVOPTION_JOB_PROOF_CREDENTIAL");
    let ca = std::fs::read(path("DEVOPTION_JOB_PROOF_CA")).unwrap();
    let address = std::env::var("DEVOPTION_JOB_PROOF_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    worker::proof_worker(&grant, &credential, &ca, address).unwrap();
}

#[tokio::test]
#[ignore = "explicit disposable PostgreSQL lease/fencing/quota proof"]
async fn manual_delivery_store() {
    let grant = Grant::load(std::path::Path::new(
        &std::env::var("DEVOPTION_JOB_PROOF_GRANT").unwrap(),
    ))
    .unwrap();
    let id = "99999999-1234-4234-8234-123456789abc";
    assert!(matches!(
        enqueue(&grant, id, "{}", 0).await,
        Ok(EnqueueOutcome::Accepted)
    ));
    assert!(matches!(
        enqueue(&grant, id, "{}", 0).await,
        Ok(EnqueueOutcome::Replayed)
    ));
    assert!(matches!(
        enqueue(&grant, id, r#"{"changed":true}"#, 0).await,
        Err(EnqueueError::Conflict)
    ));
    let first = claim(&grant).await.unwrap().unwrap();
    assert!(claim(&grant).await.unwrap().is_none());
    let (client, _driver) = grant.connect().await.unwrap();
    client.execute("UPDATE app_jobs.deliveries SET lease_until=CURRENT_TIMESTAMP-interval '1 second' WHERE app_id=$1 AND id=$2",&[&grant.app_id,&id]).await.unwrap();
    let second = claim(&grant).await.unwrap().unwrap();
    assert!(!finish(&grant, &first, Outcome::Delivered).await.unwrap());
    assert!(finish(&grant, &second, Outcome::Retry).await.unwrap());
    let delay:i64=client.query_one("SELECT EXTRACT(epoch FROM due_at-CURRENT_TIMESTAMP)::bigint FROM app_jobs.deliveries WHERE app_id=$1 AND id=$2",&[&grant.app_id,&id]).await.unwrap().get(0);
    assert!((25..=30).contains(&delay));
    for attempt in 3..=5 {
        client.execute("UPDATE app_jobs.deliveries SET due_at=CURRENT_TIMESTAMP-interval '1 second' WHERE app_id=$1 AND id=$2",&[&grant.app_id,&id]).await.unwrap();
        let lease = claim(&grant).await.unwrap().unwrap();
        assert_eq!(lease.attempts, attempt);
        assert!(finish(&grant, &lease, Outcome::Retry).await.unwrap());
    }
    let state: String = client
        .query_one(
            "SELECT state FROM app_jobs.deliveries WHERE app_id=$1 AND id=$2",
            &[&grant.app_id, &id],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(state, "needs_review");
    assert!(claim(&grant).await.unwrap().is_none());
    // Two independent clients cannot hold live leases for different jobs of one
    // app. Namespace separation does not depend on queue IDs being globally unique.
    let a = "88888888-1234-4234-8234-123456789abc";
    let b = "77777777-1234-4234-8234-123456789abc";
    enqueue(&grant, a, "{}", 0).await.unwrap();
    enqueue(&grant, b, "{}", 0).await.unwrap();
    let (left, right) = tokio::join!(claim(&grant), claim(&grant));
    assert_eq!(
        usize::from(left.unwrap().is_some()) + usize::from(right.unwrap().is_some()),
        1
    );
    let mut isolated = grant.clone();
    isolated.app_id = "e".repeat(32);
    isolated.jobs.as_mut().unwrap().project = isolated.app_id.clone();
    assert!(claim(&isolated).await.unwrap().is_none());
    // The retained terminal ledger, not only active rows, counts toward storage.
    let digest = isolated.jobs.as_ref().unwrap().digest();
    client.execute("INSERT INTO app_jobs.deliveries(app_id,id,policy,payload,delay_seconds,created_at,due_at,state) SELECT $1,'00000000-0000-4000-8000-'||lpad(n::text,12,'0'),$2,'{}',0,CURRENT_TIMESTAMP-interval '2 minutes',CURRENT_TIMESTAMP,'succeeded' FROM generate_series(1,10000)n",&[&isolated.app_id,&digest]).await.unwrap();
    assert!(matches!(
        enqueue(&isolated, a, "{}", 0).await,
        Err(EnqueueError::Unavailable)
    ));
    assert!(matches!(
        enqueue(&isolated, "00000000-0000-4000-8000-000000000001", "{}", 0).await,
        Ok(EnqueueOutcome::Replayed)
    ));
    // Deadline refusal preserves the ID and never resurrects it on replay.
    let mut expired = grant.clone();
    expired.app_id = "f".repeat(32);
    expired.jobs.as_mut().unwrap().project = expired.app_id.clone();
    enqueue(&expired, a, "{}", 0).await.unwrap();
    client.execute("UPDATE app_jobs.deliveries SET created_at=CURRENT_TIMESTAMP-interval '25 hours' WHERE app_id=$1",&[&expired.app_id]).await.unwrap();
    assert!(claim(&expired).await.unwrap().is_none());
    let state: String = client
        .query_one(
            "SELECT state FROM app_jobs.deliveries WHERE app_id=$1 AND id=$2",
            &[&expired.app_id, &a],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(state, "needs_review");
    assert!(matches!(
        enqueue(&expired, a, "{}", 0).await,
        Ok(EnqueueOutcome::Replayed)
    ));
    // Rate limits count all newly accepted IDs, including already terminal work.
    let mut rate = grant.clone();
    rate.app_id = "b".repeat(32);
    rate.jobs.as_mut().unwrap().project = rate.app_id.clone();
    let digest = rate.jobs.as_ref().unwrap().digest();
    client.execute("INSERT INTO app_jobs.deliveries(app_id,id,policy,payload,delay_seconds,due_at,state) SELECT $1,'00000000-0000-4000-8000-'||lpad(n::text,12,'0'),$2,'{}',0,CURRENT_TIMESTAMP,'succeeded' FROM generate_series(1,20)n",&[&rate.app_id,&digest]).await.unwrap();
    assert!(matches!(
        enqueue(&rate, a, "{}", 0).await,
        Err(EnqueueError::Unavailable)
    ));
}
