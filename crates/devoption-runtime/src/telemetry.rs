//! Bounded host-only HTTP observations. Snapshots contain no IPs, URLs or visitor IDs.
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    hash::BuildHasher,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone)]
pub(crate) struct ActivityCookie(pub(crate) String);
pub(crate) fn cookie_id(cookie: &str) -> Option<&str> {
    cookie
        .split(';')
        .find_map(|s| s.trim().strip_prefix("__Host-devoption-activity="))
        .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub(crate) const COOKIE: &str = "__Host-devoption-activity";

/// Host control-flow outcome, never inferred from an application's status code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    GuestResponse,
    AdmissionUnavailable,
    DeadlineExceeded,
    InvocationFailed,
    RequestRejected,
}

#[derive(Clone, Default, Serialize)]
struct Outcomes {
    guest_response: u64,
    admission_unavailable: u64,
    deadline_exceeded: u64,
    invocation_failed: u64,
    request_rejected: u64,
}
impl Outcomes {
    fn record(&mut self, outcome: Outcome) {
        *match outcome {
            Outcome::GuestResponse => &mut self.guest_response,
            Outcome::AdmissionUnavailable => &mut self.admission_unavailable,
            Outcome::DeadlineExceeded => &mut self.deadline_exceeded,
            Outcome::InvocationFailed => &mut self.invocation_failed,
            Outcome::RequestRejected => &mut self.request_rejected,
        } += 1;
    }
}
const LIMITS: [u64; 12] = [
    10,
    25,
    50,
    100,
    200,
    400,
    800,
    1600,
    3200,
    6400,
    12800,
    u64::MAX,
];
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone, Default, Serialize)]
struct Bucket {
    at: u64,
    requests: u64,
    errors: u64,
    latency: [u64; 12],
    active: usize,
    outcomes: Outcomes,
}
pub(crate) struct Telemetry {
    started: u64,
    buckets: BTreeMap<u64, Bucket>,
    visitors: HashMap<String, u64>,
    overflow_until: u64,
    last_prune: u64,
    random: std::collections::hash_map::RandomState,
    serial: u64,
}
impl Telemetry {
    pub(crate) fn new() -> Self {
        Self {
            started: now(),
            buckets: Default::default(),
            visitors: Default::default(),
            overflow_until: 0,
            last_prune: 0,
            random: Default::default(),
            serial: 0,
        }
    }
    pub(crate) fn visitor(&mut self, cookie: Option<&str>) -> (String, bool) {
        if let Some(id) =
            cookie.filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return (id.to_owned(), false);
        }
        self.serial = self.serial.wrapping_add(1);
        let a = self.random.hash_one((self.started, self.serial, 0));
        let b = self.random.hash_one((self.started, self.serial, 1));
        (format!("{a:016x}{b:016x}"), true)
    }
    /// None excludes a health probe from every counter and visitor observation.
    pub(crate) fn record(
        &mut self,
        at: u64,
        elapsed: u64,
        error: bool,
        outcome: Option<Outcome>,
        visitor: Option<String>,
    ) {
        let Some(outcome) = outcome else { return };
        self.prune(at);
        if let Some(id) = visitor {
            if self.visitors.len() < 10000 || self.visitors.contains_key(&id) {
                self.visitors.insert(id, at);
            } else {
                self.overflow_until = at.saturating_add(300);
            }
        }
        let bucket = self.buckets.entry(at / 300 * 300).or_default();
        bucket.at = at / 300 * 300;
        bucket.requests += 1;
        bucket.errors += u64::from(error);
        bucket.outcomes.record(outcome);
        bucket.latency[LIMITS.iter().position(|n| elapsed <= *n).unwrap_or(11)] += 1;
        bucket.active = self.visitors.len();
    }
    fn prune(&mut self, at: u64) {
        if at == self.last_prune {
            return;
        }
        self.last_prune = at;
        self.buckets
            .retain(|t, _| *t >= (at / 300 * 300).saturating_sub(86100));
        self.visitors.retain(|_, t| *t > at.saturating_sub(300));
    }
    fn snapshot(&mut self, at: u64, invocation: &str) -> serde_json::Value {
        self.prune(at);
        let bucket = self.buckets.entry(at / 300 * 300).or_default();
        bucket.at = at / 300 * 300;
        bucket.active = self.visitors.len();
        serde_json::json!({"version":1,"invocation":invocation,"started":self.started,"updated":at,"active":self.visitors.len(),"overflow":at < self.overflow_until,"buckets":self.buckets.values().collect::<Vec<_>>()})
    }
}
pub(crate) fn start() -> Arc<Mutex<Telemetry>> {
    let telemetry = Arc::new(Mutex::new(Telemetry::new()));
    let shared = telemetry.clone();
    tokio::spawn(async move {
        loop {
            let at = now();
            let invocation = std::env::var("INVOCATION_ID").unwrap_or_default();
            let value = shared
                .lock()
                .expect("telemetry lock")
                .snapshot(at, &invocation);
            if let Some(directory) = std::env::var_os("RUNTIME_DIRECTORY") {
                let directory = std::path::PathBuf::from(directory);
                // Only the host's preconfigured runtime directory, never a guest path.
                let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                    use std::io::Write;
                    use std::os::unix::fs::OpenOptionsExt;
                    let temp = directory.join("metrics.next");
                    let mut file = std::fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .mode(0o600)
                        .open(&temp)?;
                    let result = file
                        .write_all(serde_json::to_string(&value)?.as_bytes())
                        .and_then(|()| std::fs::rename(&temp, directory.join("metrics.json")));
                    if result.is_err() {
                        let _ = std::fs::remove_file(&temp);
                    }
                    result
                })
                .await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        }
    });
    telemetry
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outcome_counts_preserve_totals_exclude_probes_and_expire_with_buckets() {
        let mut t = Telemetry::new();
        t.started = 1000;
        for outcome in [
            Outcome::GuestResponse,
            Outcome::AdmissionUnavailable,
            Outcome::DeadlineExceeded,
            Outcome::InvocationFailed,
            Outcome::RequestRejected,
        ] {
            let error = !matches!(outcome, Outcome::GuestResponse | Outcome::RequestRejected);
            t.record(1000, 20, error, Some(outcome), None);
        }
        t.record(1001, 20, true, Some(Outcome::GuestResponse), None);
        let before = t.snapshot(1002, "worker");
        let bucket = &before["buckets"][0];
        assert_eq!(bucket["requests"], 6);
        assert_eq!(bucket["errors"], 4);
        assert_eq!(bucket["latency"][1], 6);
        assert_eq!(
            bucket["outcomes"],
            serde_json::json!({
                "guest_response": 2, "admission_unavailable": 1,
                "deadline_exceeded": 1, "invocation_failed": 1, "request_rejected": 1,
            })
        );
        // Both healthy and failed probes leave all counters and users untouched.
        for error in [false, true] {
            t.record(1002, 5000, error, None, Some("excluded-probe".into()));
        }
        assert_eq!(t.snapshot(1002, "worker"), before);
        t.record(1300, 20, false, Some(Outcome::GuestResponse), None);
        let next = t.snapshot(1300, "worker");
        assert_eq!(next["buckets"][1]["requests"], 1);
        assert_eq!(next["buckets"][1]["outcomes"]["guest_response"], 1);
        let expired = t.snapshot(100000, "worker");
        assert_eq!(expired["buckets"].as_array().unwrap().len(), 1);
        assert_eq!(expired["buckets"][0]["requests"], 0);
        assert!(
            expired["buckets"][0]["outcomes"]
                .as_object()
                .unwrap()
                .values()
                .all(|n| n == 0)
        );
    }

    #[test]
    fn observations_expire_and_do_not_export_identifiers() {
        let mut t = Telemetry::new();
        t.started = 1000;
        t.record(
            1000,
            151,
            false,
            Some(Outcome::GuestResponse),
            Some("private-browser-id".into()),
        );
        t.record(
            1001,
            42,
            true,
            Some(Outcome::GuestResponse),
            Some("private-browser-id".into()),
        );
        let s = t.snapshot(1002, "worker");
        assert_eq!(s["active"], 1);
        assert_eq!(s["buckets"][0]["requests"], 2);
        assert_eq!(s["buckets"][0]["errors"], 1);
        assert!(!s.to_string().contains("private-browser-id"));
        assert_eq!(t.snapshot(1301, "worker")["active"], 0);
        assert_eq!(
            t.snapshot(100000, "worker")["buckets"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn activity_memory_is_bounded_and_saturation_expires() {
        let mut t = Telemetry::new();
        for i in 0..10001 {
            t.record(
                1000,
                1,
                false,
                Some(Outcome::GuestResponse),
                Some(format!("visitor-{i}")),
            );
        }
        assert_eq!(t.visitors.len(), 10000);
        assert_eq!(t.snapshot(1000, "worker")["overflow"], true);
        assert_eq!(t.snapshot(1301, "worker")["overflow"], false);
        assert_eq!(t.visitors.len(), 0);
    }
    #[test]
    fn visitor_ids_are_validated_and_fresh() {
        let mut t = Telemetry::new();
        let (id, fresh) = t.visitor(None);
        assert!(fresh);
        assert_eq!(id.len(), 32);
        assert_ne!(id, t.visitor(None).0);
        assert_eq!(
            t.visitor(cookie_id(&format!("other=1; {COOKIE}={id}"))),
            (id, false)
        );
        assert!(t.visitor(Some("__Host-devoption-activity=bad")).1);
    }
}
