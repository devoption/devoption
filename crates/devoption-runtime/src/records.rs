//! Experimental, explicit host grant. Never receives a guest-selected SQL/DSN.
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, time::Duration};
use tokio_postgres::NoTls;

wasmtime::component::bindgen!({
    path: "wit/records.wit",
    world: "record-imports",
    imports: { default: async },
});
pub use devoption::app::records::{WriteError, WriteOutcome};

const MAX_PAYLOAD: usize = 24 * 1024;
const DB_DEADLINE: Duration = Duration::from_secs(2);
// Persistent per-namespace bounds survive worker restart. Replays never consume
// quota; no automatic deletion is used to make space for untrusted submissions.
const MAX_RECORDS: i64 = 10_000;
const NEW_PER_MINUTE: i64 = 20;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    app_id: String,
    collection: String,
    socket_dir: String,
    database: String,
    user: String,
    fields: BTreeMap<String, Field>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    required: bool,
    max_chars: usize,
    #[serde(default)]
    multiline: bool,
    #[serde(default)]
    email: bool,
    #[serde(default)]
    choices: Vec<String>,
}

fn identifier(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

fn email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
        && domain.contains('.')
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

// serde_json::Value normally accepts duplicate object keys. Do not let an
// ambiguous request have different validation/replay meaning in other clients.
struct UniqueObject(BTreeMap<String, Value>);
impl<'de> Deserialize<'de> for UniqueObject {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueObject;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unique object keys")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut a: A,
            ) -> Result<Self::Value, A::Error> {
                let mut fields = BTreeMap::new();
                while let Some((k, v)) = a.next_entry::<String, Value>()? {
                    if fields.insert(k, v).is_some() {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                }
                Ok(UniqueObject(fields))
            }
        }
        d.deserialize_map(Visitor)
    }
}

impl Grant {
    pub fn load(path: &Path) -> wasmtime::Result<Self> {
        use std::io::Read;
        let file = std::fs::File::open(path)?;
        wasmtime::ensure!(
            file.metadata()?.is_file() && file.metadata()?.len() <= 32768,
            "invalid record grant"
        );
        let mut bytes = Vec::new();
        file.take(32769).read_to_end(&mut bytes)?;
        // Never echo configuration or serde diagnostics (which may quote values).
        let grant: Self = serde_json::from_slice(&bytes)
            .map_err(|_| wasmtime::format_err!("invalid record grant"))?;
        wasmtime::ensure!(grant.valid(), "invalid record grant");
        Ok(grant)
    }

    fn valid(&self) -> bool {
        self.app_id.len() == 32
            && self
                .app_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && identifier(&self.collection, 64)
            && identifier(&self.database, 63)
            && identifier(&self.user, 63)
            && Path::new(&self.socket_dir).is_absolute()
            && self.socket_dir.len() <= 80
            && !self.fields.is_empty()
            && self.fields.len() <= 16
            && self.fields.iter().all(|(k, f)| {
                identifier(k, 64)
                    && (1..=4000).contains(&f.max_chars)
                    && !(f.email && f.multiline)
                    && f.choices.len() <= 32
                    && f.choices.iter().all(|s| {
                        !s.is_empty()
                            && s.trim() == s
                            && s.chars().count() <= f.max_chars
                            && !s.chars().any(char::is_control)
                    })
            })
    }

    fn validate(&self, id: &str, input: &str) -> Result<Value, WriteError> {
        if !uuid(id) || input.len() > MAX_PAYLOAD {
            return Err(WriteError::Invalid);
        }
        let UniqueObject(fields) = serde_json::from_str(input).map_err(|_| WriteError::Invalid)?;
        if fields.keys().any(|k| !self.fields.contains_key(k)) {
            return Err(WriteError::Invalid);
        }
        let mut normalized = serde_json::Map::new();
        for (key, schema) in &self.fields {
            let raw = match fields.get(key) {
                None | Some(Value::Null) => "",
                Some(Value::String(s)) => s,
                _ => return Err(WriteError::Invalid),
            };
            // Check raw controls before trim, so leading newlines are not hidden.
            if raw
                .chars()
                .any(|c| c.is_control() && !(schema.multiline && ['\n', '\r', '\t'].contains(&c)))
            {
                return Err(WriteError::Invalid);
            }
            let value = raw.trim();
            if value.is_empty() {
                if schema.required {
                    return Err(WriteError::Invalid);
                }
                normalized.insert(key.clone(), Value::Null);
            } else {
                if value.chars().count() > schema.max_chars
                    || (schema.email && !email(value))
                    || (!schema.choices.is_empty() && !schema.choices.iter().any(|s| s == value))
                {
                    return Err(WriteError::Invalid);
                }
                normalized.insert(key.clone(), Value::String(value.to_owned()));
            }
        }
        Ok(Value::Object(normalized))
    }

    pub async fn append(&self, id: &str, input: &str) -> Result<WriteOutcome, WriteError> {
        let payload = self.validate(id, input)?;
        tokio::time::timeout(DB_DEADLINE, self.persist(id, payload))
            .await
            .unwrap_or(Err(WriteError::Unavailable))
    }

    async fn persist(&self, id: &str, payload: Value) -> Result<WriteOutcome, WriteError> {
        let mut config = tokio_postgres::Config::new();
        config
            .host_path(&self.socket_dir)
            .dbname(&self.database)
            .user(&self.user)
            .connect_timeout(Duration::from_millis(500));
        // Local peer-authenticated Unix socket only. No DSN, password, environment
        // lookup or plaintext remote TCP fallback. No guest connection selection.
        let (mut client, connection) = config
            .connect(NoTls)
            .await
            .map_err(|_| WriteError::Unavailable)?;
        struct Driver(tokio::task::JoinHandle<()>);
        impl Drop for Driver {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _driver = Driver(tokio::spawn(async move {
            let _ = connection.await;
        }));
        let work = async {
            let tx = client.transaction().await?;
            tx.batch_execute("SET LOCAL synchronous_commit = on; SET LOCAL statement_timeout = '1000ms'; SET LOCAL lock_timeout = '500ms'").await?;
            let settings = tx.query_one("SELECT current_setting('fsync'), current_setting('full_page_writes'), relpersistence::text FROM pg_catalog.pg_class WHERE oid = 'app_records.entries'::regclass", &[]).await?;
            if settings.get::<_,String>(0) != "on" || settings.get::<_,String>(1) != "on" || settings.get::<_,String>(2) != "p" {
                return Ok::<_,tokio_postgres::Error>(Err(WriteError::Unavailable));
            }
            tx.query_one("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended($1::text || ':' || $2::text, 0))", &[&self.app_id,&self.collection]).await?;
            if let Some(row) = tx.query_opt("SELECT payload FROM app_records.entries WHERE app_id=$1 AND collection=$2 AND id=$3", &[&self.app_id,&self.collection,&id]).await? {
                if row.get::<_,Value>(0) != payload { return Ok(Err(WriteError::Conflict)); }
                tx.commit().await?;
                return Ok(Ok(WriteOutcome::Replayed));
            }
            let budget = tx.query_one("SELECT count(*), count(*) FILTER (WHERE created_at >= CURRENT_TIMESTAMP - interval '1 minute') FROM app_records.entries WHERE app_id=$1 AND collection=$2", &[&self.app_id,&self.collection]).await?;
            if budget.get::<_,i64>(0) >= MAX_RECORDS || budget.get::<_,i64>(1) >= NEW_PER_MINUTE {
                return Ok(Err(WriteError::Unavailable));
            }
            let inserted = tx.execute("INSERT INTO app_records.entries (app_id, collection, id, payload) VALUES ($1,$2,$3,$4) ON CONFLICT (app_id,collection,id) DO NOTHING", &[&self.app_id,&self.collection,&id,&payload]).await?;
            let outcome = if inserted == 1 { WriteOutcome::Created } else {
                let row = tx.query_one("SELECT payload FROM app_records.entries WHERE app_id=$1 AND collection=$2 AND id=$3", &[&self.app_id,&self.collection,&id]).await?;
                if row.get::<_,Value>(0) != payload { return Ok(Err(WriteError::Conflict)); }
                WriteOutcome::Replayed
            };
            tx.commit().await?;
            Ok(Ok(outcome))
        }.await;
        // Error text can contain submitted values, SQL or server configuration.
        work.unwrap_or(Err(WriteError::Unavailable))
    }
}

impl devoption::app::records::Host for super::State {
    async fn append(&mut self, id: String, payload: String) -> Result<WriteOutcome, WriteError> {
        if self.record_call_used {
            return Err(WriteError::Denied);
        }
        self.record_call_used = true;
        match &self.records {
            Some(grant) => grant.append(&id, &payload).await,
            None => Err(WriteError::Denied),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "12345678-1234-4234-8234-123456789abc";
    fn grant() -> Grant {
        serde_json::from_value(serde_json::json!({
            "app_id":"0123456789abcdef0123456789abcdef", "collection":"notes",
            "socket_dir":"/nonexistent-records-proof", "database":"records", "user":"worker",
            "fields": {
                "label":{"required":true,"max_chars":20},
                "note":{"required":false,"max_chars":100,"multiline":true},
                "reply":{"required":false,"max_chars":254,"email":true},
                "category":{"required":false,"max_chars":20,"choices":["general","other"]}
            }
        }))
        .unwrap()
    }
    #[test]
    fn normalizes_for_retries_without_silently_dropping_fields() {
        let g = grant();
        assert!(g.valid());
        let a = g.validate(ID, r#"{"label":" hello ","note":" "}"#).unwrap();
        let b = g.validate(ID, r#"{"note":null,"label":"hello"}"#).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["reply"], Value::Null);
        assert_eq!(
            g.validate(ID, r#"{"label":"hello","note":"line1\nline2"}"#)
                .unwrap()["note"],
            "line1\nline2"
        );
    }
    #[test]
    fn rejects_ambiguous_invalid_or_ungranted_values() {
        for input in [
            r#"{"label":""}"#,
            r#"{"label":"a","label":"b"}"#,
            r#"{"label":"x","sql":"SELECT 1"}"#,
            r#"{"label":1}"#,
            r#"{"label":{"nested":true}}"#,
            r#"{"label":"\nhello"}"#,
            r#"{"label":"x","note":"\u0000"}"#,
            r#"{"label":"x","reply":"bad@host"}"#,
            r#"{"label":"x","reply":"a@@b.test"}"#,
            r#"{"label":"x","category":"unknown"}"#,
            r#"[]"#,
            r#"{"label":"x"} {}"#,
        ] {
            assert!(
                matches!(grant().validate(ID, input), Err(WriteError::Invalid)),
                "{input}"
            );
        }
        for id in ["", "../../store", "12345678-1234-4234-8234-123456789ABC"] {
            assert!(grant().validate(id, r#"{"label":"hello"}"#).is_err());
        }
        assert!(grant().validate(ID, &" ".repeat(MAX_PAYLOAD + 1)).is_err());
    }
    #[test]
    fn invalid_grants_cannot_select_remote_transport_or_unbounded_schema() {
        let mut g = grant();
        g.socket_dir = "localhost".into();
        assert!(!g.valid());
        let mut g = grant();
        g.database = "records options=-c".into();
        assert!(!g.valid());
        let mut g = grant();
        g.fields.get_mut("label").unwrap().max_chars = 4001;
        assert!(!g.valid());
        let mut g = grant();
        g.fields.get_mut("reply").unwrap().multiline = true;
        assert!(!g.valid());
    }
    #[tokio::test]
    async fn database_failure_never_reports_success() {
        assert!(matches!(
            grant().append(ID, r#"{"label":"hello"}"#).await,
            Err(WriteError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn request_grant_is_write_only_and_single_use_even_after_invalid_input() {
        use devoption::app::records::Host;
        let mut state = super::super::State {
            wasi: wasmtime_wasi::WasiCtx::builder().build(),
            http: wasmtime_wasi_http::WasiHttpCtx::new(),
            table: wasmtime::component::ResourceTable::new(),
            limits: wasmtime::StoreLimitsBuilder::new().build(),
            hooks: super::super::DenyNetwork,
            records: Some(std::sync::Arc::new(grant())),
            record_call_used: false,
        };
        assert!(matches!(
            state.append(ID.into(), "{}".into()).await,
            Err(WriteError::Invalid)
        ));
        assert!(matches!(
            state.append(ID.into(), r#"{"label":"hello"}"#.into()).await,
            Err(WriteError::Denied)
        ));
        state.records = None;
        state.record_call_used = false;
        assert!(matches!(
            state.append(ID.into(), r#"{"label":"hello"}"#.into()).await,
            Err(WriteError::Denied)
        ));
    }
}
