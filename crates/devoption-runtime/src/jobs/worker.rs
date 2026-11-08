//! Explicit separate operator process; never runs in the HTTP request handler.
use super::*;
use std::{
    io::Read,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;

fn snapshot(path: &Path) -> Result<Zeroizing<Vec<u8>>, ()> {
    let file = std::fs::File::open(path).map_err(|_| ())?;
    if !file.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(32769).read_to_end(&mut bytes).map_err(|_| ())?;
    if bytes.len() > 32768 {
        return Err(());
    }
    Ok(bytes)
}
fn fingerprint(path: &Path) -> Result<Vec<u8>, ()> {
    Ok(Sha256::digest(&*snapshot(path)?).to_vec())
}

pub(crate) async fn step(
    grant: &Grant,
    sender: &crate::https::Grant,
    stop: &AtomicBool,
) -> Result<bool, ()> {
    let policy = grant.jobs.as_ref().ok_or(())?;
    if !sender.matches_delivery(policy) || stop.load(Ordering::SeqCst) {
        return Err(());
    }
    let Some(lease) = super::claim(grant).await? else {
        return Ok(false);
    };
    if stop.load(Ordering::SeqCst) {
        return Ok(true);
    }
    // Recheck fencing immediately before sending. A suspension after this check
    // can still race with cancellation/expiry; receiver deduplication is required.
    let valid=tokio::time::timeout(DEADLINE,async {
        let (client,_driver)=grant.connect().await?;
        client.query_one("SELECT EXISTS(SELECT 1 FROM app_jobs.deliveries WHERE app_id=$1 AND id=$2 AND generation=$3 AND state='leased' AND lease_until>CURRENT_TIMESTAMP)",&[&grant.app_id,&lease.id,&lease.generation]).await
    }).await.map_err(|_|())?.map_err(|_|())?.get::<_,bool>(0);
    if !valid {
        return Ok(true);
    }
    let key = format!("{}:{}", grant.app_id, lease.id);
    let outcome = match sender.deliver(&key, &lease.payload).await {
        Ok(status) if (200..300).contains(&status) => Outcome::Delivered,
        Ok(status) if (400..500).contains(&status) && ![408, 429].contains(&status) => {
            Outcome::Rejected
        }
        _ => Outcome::Retry,
    };
    super::finish(grant, &lease, outcome).await?;
    Ok(true)
}

/// Foreground opt-in worker. The operator/systemd owns service count, identity,
/// filesystem/network limits and credential delivery. It starts no app or child.
pub fn run(grant_path: &Path, credential_path: &Path) -> Result<(), String> {
    run_with_sender(grant_path, credential_path, crate::https::Grant::from_bytes)
}

fn run_with_sender(
    grant_path: &Path,
    credential_path: &Path,
    make: impl FnOnce(&[u8]) -> wasmtime::Result<crate::https::Grant>,
) -> Result<(), String> {
    let work = || -> Result<(), ()> {
        let grant_bytes = snapshot(grant_path)?;
        let credential_bytes = snapshot(credential_path)?;
        let grant = Grant::from_bytes(&grant_bytes).map_err(|_| ())?;
        let sender = make(&credential_bytes).map_err(|_| ())?;
        if !sender.matches_delivery(grant.jobs.as_ref().ok_or(())?) {
            return Err(());
        }
        let grant_hash = Sha256::digest(&*grant_bytes).to_vec();
        let credential_hash = Sha256::digest(&*credential_bytes).to_vec();
        drop((grant_bytes, credential_bytes));
        let stop = Arc::new(AtomicBool::new(false));
        let signals = [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT]
            .map(|signal| signal_hook::flag::register(signal, stop.clone()));
        let registrations = signals
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ())?;
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ())?
            .block_on(async {
                while !stop.load(Ordering::SeqCst) {
                    if fingerprint(grant_path)? != grant_hash
                        || fingerprint(credential_path)? != credential_hash
                    {
                        return Err(());
                    }
                    // Storage failures remain durable/unavailable; do not claim success
                    // or perform a network call without a freshly committed lease.
                    let _ = step(&grant, &sender, &stop).await;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Ok(())
            });
        for id in registrations {
            signal_hook::low_level::unregister(id);
        }
        result
    };
    work().map_err(|_| "delivery worker unavailable or policy changed".into())
}

#[cfg(test)]
pub(crate) fn proof_worker(
    grant: &Path,
    credential: &Path,
    ca: &[u8],
    address: std::net::SocketAddr,
) -> Result<(), String> {
    run_with_sender(grant, credential, |bytes| {
        crate::https::delivery_fixture(bytes, ca, address)
    })
}

/// Local operator observation/cancellation. Never includes payloads or secrets.
pub fn control(path: &Path, cancel: Option<&str>) -> Result<String, String> {
    let work = || -> Result<String, ()> {
        let grant = Grant::load(path).map_err(|_| ())?;
        grant.jobs.as_ref().ok_or(())?;
        if cancel.is_some_and(|id| !crate::records::uuid(id)) {
            return Err(());
        }
        tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|())?.block_on(async {
            tokio::time::timeout(DEADLINE,async {
                let (mut client,_driver)=grant.connect().await?;
                let tx=client.transaction().await?;
                tx.batch_execute(SETTINGS).await?;
                if !durable(&tx).await? {return Ok(Err(()));}
                let mut changed=false;
                if let Some(id)=cancel {
                    changed=tx.execute("UPDATE app_jobs.deliveries SET state=CASE WHEN state='leased' THEN 'needs_review' ELSE 'cancelled' END,outcome=CASE WHEN state='leased' THEN 'cancel_ambiguous' ELSE 'cancelled' END,generation=generation+1 WHERE app_id=$1 AND id=$2 AND state IN ('queued','retry_wait','leased')",&[&grant.app_id,&id]).await? != 0;
                }
                let counts=tx.query("SELECT state,count(*) FROM app_jobs.deliveries WHERE app_id=$1 GROUP BY state ORDER BY state",&[&grant.app_id]).await?.into_iter().map(|r|(r.get::<_,String>(0),r.get::<_,i64>(1))).collect::<std::collections::BTreeMap<_,_>>();
                let rows=tx.query("SELECT id,state,attempts,outcome,created_at::text,due_at::text,COALESCE(lease_until::text,''),policy FROM app_jobs.deliveries WHERE app_id=$1 AND state NOT IN ('succeeded','cancelled') ORDER BY created_at,id LIMIT 100",&[&grant.app_id]).await?.into_iter().map(|r|{
                    let code:String=r.get(3);
                    let code=if ["pending","in_flight","policy_or_deadline","stored_payload","provider_rejected","attempts_exhausted","delivery_uncertain","cancel_ambiguous"].contains(&code.as_str()){code.as_str()}else{"unknown"};
                    serde_json::json!({"id":r.get::<_,String>(0),"state":r.get::<_,String>(1),"attempts":r.get::<_,i32>(2),"outcome":code,"created_at":r.get::<_,String>(4),"due_at":r.get::<_,String>(5),"lease_until":r.get::<_,String>(6),"policy":r.get::<_,String>(7)})
                }).collect::<Vec<_>>();
                tx.commit().await?;
                Ok::<_,tokio_postgres::Error>(serde_json::to_string(&serde_json::json!({"counts":counts,"exceptions_and_pending":rows,"detail_limit":100,"current_policy":grant.jobs.as_ref().unwrap().digest(),"cancellation":cancel.map(|id|serde_json::json!({"id":id,"updated":changed}))})).map_err(|_|()))
            }).await.map_err(|_|())?.map_err(|_|())?
        })
    };
    work().map_err(|_| "delivery job observation/control unavailable".into())
}
