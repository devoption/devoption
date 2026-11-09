//! Bounded durable HTTPS delivery. No arbitrary job code, SQL or destinations.
use crate::records::Grant;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::Duration;

wasmtime::component::bindgen!({
    path: "wit/records.wit",
    world: "delivery-imports",
    imports: { default: async },
});
pub use devoption::app::delivery_jobs::{EnqueueError, EnqueueOutcome};

/// Value-free policy; credentials are delivered only to the separate dispatcher.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Policy {
    pub project: String,
    pub environment: String,
    pub secret: String,
    pub revision: u64,
    pub endpoint: String,
    pub receiver_dedup_seconds: u32,
}
impl Policy {
    pub(crate) fn valid(&self, app: &str) -> bool {
        self.project == app
            && ["development", "preview", "production"].contains(&self.environment.as_str())
            && self.revision > 0
            && !self.secret.is_empty()
            && self.secret.len() <= 64
            && self
                .secret
                .bytes()
                .enumerate()
                .all(|(i, b)| b.is_ascii_uppercase() || b == b'_' || (i > 0 && b.is_ascii_digit()))
            && crate::https::endpoint(&self.endpoint).is_some()
            && self.receiver_dedup_seconds >= 172800
    }
    pub(crate) fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("fixed serializable policy");
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

const DEADLINE: Duration = Duration::from_secs(2);
const SETTINGS: &str = "SET LOCAL synchronous_commit=on; SET LOCAL statement_timeout='1000ms'; SET LOCAL lock_timeout='500ms'";

async fn durable(tx: &tokio_postgres::Transaction<'_>) -> Result<bool, tokio_postgres::Error> {
    let row = tx.query_one("SELECT current_setting('fsync'),current_setting('full_page_writes'),relpersistence::text FROM pg_catalog.pg_class WHERE oid='app_jobs.deliveries'::regclass",&[]).await?;
    Ok(row.get::<_, String>(0) == "on"
        && row.get::<_, String>(1) == "on"
        && row.get::<_, String>(2) == "p")
}

fn payload(raw: &str) -> Result<Value, EnqueueError> {
    if raw.len() > 24 * 1024 {
        return Err(EnqueueError::Invalid);
    }
    let value = serde_json::from_str::<UniqueJson>(raw)
        .map_err(|_| EnqueueError::Invalid)?
        .0;
    if !value.is_object()
        || serde_json::to_vec(&value)
            .map_err(|_| EnqueueError::Invalid)?
            .len()
            > 24576
    {
        return Err(EnqueueError::Invalid);
    }
    Ok(value)
}

struct UniqueJson(Value);
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| UniqueJson(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(v.into()))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(UniqueJson(v)) = a.next_element()? {
                    items.push(v);
                }
                Ok(UniqueJson(Value::Array(items)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut items = serde_json::Map::new();
                while let Some((k, UniqueJson(v))) = a.next_entry::<String, UniqueJson>()? {
                    if items.insert(k, v).is_some() {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                }
                Ok(UniqueJson(Value::Object(items)))
            }
        }
        d.deserialize_any(Visitor)
    }
}

pub(crate) async fn enqueue(
    grant: &Grant,
    id: &str,
    raw: &str,
    delay: u32,
) -> Result<EnqueueOutcome, EnqueueError> {
    let policy = grant.jobs.as_ref().ok_or(EnqueueError::Denied)?;
    if !crate::records::uuid(id) || delay > 3600 {
        return Err(EnqueueError::Invalid);
    }
    let value = payload(raw)?;
    let digest = policy.digest();
    tokio::time::timeout(DEADLINE,async {
        let (mut client,_driver)=grant.connect().await?;
        let tx=client.transaction().await?;
        tx.batch_execute(SETTINGS).await?;
        if !durable(&tx).await? { return Ok(Err(EnqueueError::Unavailable)); }
        tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1,296))",&[&grant.app_id]).await?;
        if let Some(row)=tx.query_opt("SELECT policy,payload,delay_seconds FROM app_jobs.deliveries WHERE app_id=$1 AND id=$2",&[&grant.app_id,&id]).await? {
            let same=row.get::<_,String>(0)==digest && row.get::<_,Value>(1)==value && row.get::<_,i32>(2)==delay as i32;
            tx.commit().await?;
            return Ok(if same {Ok(EnqueueOutcome::Replayed)} else {Err(EnqueueError::Conflict)});
        }
        let row=tx.query_one("SELECT count(*),count(*) FILTER (WHERE created_at>=CURRENT_TIMESTAMP-interval '1 minute') FROM app_jobs.deliveries WHERE app_id=$1",&[&grant.app_id]).await?;
        if row.get::<_,i64>(0)>=10000 || row.get::<_,i64>(1)>=20 {return Ok(Err(EnqueueError::Unavailable));}
        tx.execute("INSERT INTO app_jobs.deliveries(app_id,id,policy,payload,delay_seconds,due_at,state) VALUES($1,$2,$3,$4,$5,CURRENT_TIMESTAMP+make_interval(secs=>$5::integer),'queued')",&[&grant.app_id,&id,&digest,&value,&(delay as i32)]).await?;
        tx.commit().await?;
        Ok::<_,tokio_postgres::Error>(Ok(EnqueueOutcome::Accepted))
    }).await.map_err(|_|EnqueueError::Unavailable)?.unwrap_or(Err(EnqueueError::Unavailable))
}

pub(crate) struct Lease {
    pub id: String,
    pub generation: i64,
    pub attempts: i32,
    pub payload: Vec<u8>,
}

pub(crate) async fn claim(grant: &Grant) -> Result<Option<Lease>, ()> {
    let policy = grant.jobs.as_ref().ok_or(())?.digest();
    tokio::time::timeout(DEADLINE,async {
        let (mut client,_driver)=grant.connect().await?;
        let tx=client.transaction().await?;
        tx.batch_execute(SETTINGS).await?;
        if !durable(&tx).await? {return Ok(Err(()));}
        tx.query_one("SELECT pg_advisory_xact_lock(hashtextextended($1,296))",&[&grant.app_id]).await?;
        tx.execute("UPDATE app_jobs.deliveries SET state='needs_review',outcome='policy_or_deadline',generation=generation+1 WHERE app_id=$1 AND state IN ('queued','retry_wait','leased') AND (policy<>$2 OR created_at<CURRENT_TIMESTAMP-interval '24 hours' OR (state='leased' AND lease_until<=CURRENT_TIMESTAMP AND attempts>=5))",&[&grant.app_id,&policy]).await?;
        // A live lease prevents parallel delivery for this app, including when
        // independent dispatcher processes compete for different rows.
        let busy=tx.query_one("SELECT EXISTS(SELECT 1 FROM app_jobs.deliveries WHERE app_id=$1 AND lease_until>CURRENT_TIMESTAMP)",&[&grant.app_id]).await?.get::<_,bool>(0);
        let row=if busy {None} else {tx.query_opt("SELECT id,attempts,generation,CASE WHEN octet_length(payload::text)<=65536 THEN payload::text ELSE NULL END FROM app_jobs.deliveries WHERE app_id=$1 AND policy=$2 AND attempts<5 AND ((state IN ('queued','retry_wait') AND due_at<=CURRENT_TIMESTAMP) OR (state='leased' AND lease_until<=CURRENT_TIMESTAMP)) ORDER BY due_at,id LIMIT 1 FOR UPDATE SKIP LOCKED",&[&grant.app_id,&policy]).await?};
        let lease=if let Some(row)=row {
            let id:String=row.get(0);
            let generation:i64=row.get::<_,i64>(2)+1;
            let attempts:i32=row.get::<_,i32>(1)+1;
            match row.get::<_,Option<String>>(3).and_then(|s|serde_json::from_str::<Value>(&s).ok()).filter(Value::is_object).and_then(|v|serde_json::to_vec(&v).ok()).filter(|v|v.len()<=24576) {
                Some(payload)=>{
                    tx.execute("UPDATE app_jobs.deliveries SET state='leased',attempts=$3,generation=$4,lease_until=CURRENT_TIMESTAMP+interval '30 seconds',outcome='in_flight' WHERE app_id=$1 AND id=$2",&[&grant.app_id,&id,&attempts,&generation]).await?;
                    Some(Lease{id,generation,attempts,payload})
                }
                None=>{
                    tx.execute("UPDATE app_jobs.deliveries SET state='needs_review',outcome='stored_payload',generation=generation+1 WHERE app_id=$1 AND id=$2",&[&grant.app_id,&id]).await?;
                    None
                }
            }
        } else {None};
        tx.commit().await?;
        Ok::<_,tokio_postgres::Error>(Ok(lease))
    }).await.map_err(|_|())?.map_err(|_|())?
}

#[derive(Clone, Copy)]
pub(crate) enum Outcome {
    Delivered,
    Retry,
    Rejected,
}

pub(crate) async fn finish(grant: &Grant, lease: &Lease, outcome: Outcome) -> Result<bool, ()> {
    let (state, code, delay) = match outcome {
        Outcome::Delivered => ("succeeded", "delivered", 0),
        Outcome::Rejected => ("failed", "provider_rejected", 0),
        Outcome::Retry if lease.attempts >= 5 => ("needs_review", "attempts_exhausted", 0),
        Outcome::Retry => (
            "retry_wait",
            "delivery_uncertain",
            [5, 30, 120, 600][(lease.attempts - 1) as usize],
        ),
    };
    tokio::time::timeout(DEADLINE,async {
        let (mut client,_driver)=grant.connect().await?;
        let tx=client.transaction().await?;tx.batch_execute(SETTINGS).await?;
        if !durable(&tx).await? {return Ok(false);}
        let count=tx.execute("UPDATE app_jobs.deliveries SET state=$4,outcome=$5,due_at=CURRENT_TIMESTAMP+make_interval(secs=>$6::integer),lease_until=NULL WHERE app_id=$1 AND id=$2 AND generation=$3 AND state='leased' AND lease_until>CURRENT_TIMESTAMP",&[&grant.app_id,&lease.id,&lease.generation,&state,&code,&delay]).await?;
        tx.commit().await?;
        Ok::<_,tokio_postgres::Error>(count==1)
    }).await.map_err(|_|())?.map_err(|_|())
}

impl devoption::app::delivery_jobs::Host for crate::State {
    async fn enqueue(
        &mut self,
        id: String,
        payload: String,
        delay_seconds: u32,
    ) -> Result<EnqueueOutcome, EnqueueError> {
        if self.record_call_used {
            return Err(EnqueueError::Denied);
        }
        self.record_call_used = true;
        match &self.records {
            Some(grant) => enqueue(grant, &id, &payload, delay_seconds).await,
            None => Err(EnqueueError::Denied),
        }
    }
}

pub mod worker;

#[cfg(test)]
mod tests;
