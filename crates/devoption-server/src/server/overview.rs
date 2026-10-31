//! Read-only catalog/history/runtime aggregation. No workload starts or approvals.
use super::*;
use devoption_ui::{MetricPoint, Monitoring, Overview, OverviewOutcome, OverviewProject};
use serde_json::Value;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
const LATENCY: [u64; 12] = [
    10, 25, 50, 100, 200, 400, 800, 1600, 3200, 6400, 12800, 12800,
];
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn range(query: &Option<String>) -> u64 {
    if query
        .as_deref()
        .is_some_and(|s| s.split('&').any(|v| v == "range=1h"))
    {
        1
    } else {
        24
    }
}

pub(super) async fn home(
    projects: Option<projects::Projects>,
    installed: bool,
    query: Option<String>,
) -> axum::response::Response {
    let hours = range(&query);
    let view = match projects {
        Some(projects) => snapshot(projects, hours).await,
        None => Overview {
            range_hours: hours,
            ..Default::default()
        },
    };
    let updates = tokio::task::spawn_blocking(move || platform_updates::observation(installed))
        .await
        .unwrap_or_else(|_| devoption_ui::PlatformUpdates {
            unavailable: true,
            ..Default::default()
        });
    (
        [(CACHE_CONTROL, "no-store")],
        Html(devoption_ui::render_overview(&view, &updates)),
    )
        .into_response()
}
pub(super) async fn project(
    projects: projects::Projects,
    id: devoption_projects::ProjectId,
    query: Option<String>,
) -> axum::response::Response {
    let hours = range(&query);
    let view = snapshot_scoped(projects, hours, Some(id.to_bytes())).await;
    let Some(p) = view.projects.iter().find(|p| p.id == id.to_bytes()) else {
        return (
            if view.catalog_available {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            },
            [(CACHE_CONTROL, "no-store")],
            "Project monitoring is unavailable.",
        )
            .into_response();
    };
    let monitoring = &view.monitoring;
    let project = devoption_ui::ProjectSummary {
        id: p.id,
        name: &p.name,
        repositories: vec![],
    };
    (
        [(CACHE_CONTROL, "no-store")],
        Html(devoption_ui::render_project_monitoring(
            &project, monitoring, hours,
        )),
    )
        .into_response()
}
async fn runtime(projects: &projects::Projects) -> Option<Value> {
    projects.deployment.as_ref()?;
    let mut cache = projects.overview_cache.lock().await;
    if let Some((at, value)) = &*cache
        && at.elapsed().as_secs() < 15
    {
        return Some(value.clone());
    }
    let value = tokio::task::spawn_blocking(|| projects::deployment::broker(&["overview"]))
        .await
        .ok()?
        .ok()?;
    *cache = Some((Instant::now(), value.clone()));
    Some(value)
}
async fn snapshot(projects: projects::Projects, hours: u64) -> Overview {
    snapshot_scoped(projects, hours, None).await
}
async fn snapshot_scoped(
    projects: projects::Projects,
    hours: u64,
    scope: Option<[u8; 16]>,
) -> Overview {
    let runtime = runtime(&projects).await;
    let history = match projects.history.clone() {
        Some(h) => Some(h.lock_owned().await),
        None => None,
    };
    let gate = projects.gate.clone().lock_owned().await;
    tokio::task::spawn_blocking(move || {
        let _gate = gate;
        let mut view = Overview {
            range_hours: hours,
            deployment_available: runtime.is_some(),
            ..Default::default()
        };
        let Ok(catalog) = projects.directory.load() else {
            return view;
        };
        let catalog = catalog.unwrap_or_default();
        view.catalog_available = true;
        let records = history.as_ref().and_then(|h| h.load().ok());
        view.history_available = records.is_some();
        let records = records.unwrap_or_default();
        // History associations must still exist in the fresh catalog.
        if records.iter().any(|r| {
            !catalog.projects().iter().any(|p| {
                p.id().to_bytes() == r.project()
                    && p.repositories()
                        .iter()
                        .any(|id| id.to_bytes() == r.repository())
            })
        }) {
            view.history_available = false;
        }
        let apps = runtime.as_ref().and_then(|v| v["apps"].as_array());
        let mut telemetry = Vec::new();
        let at = now();
        for p in catalog
            .projects()
            .iter()
            .filter(|p| scope.is_none_or(|id| id == p.id().to_bytes()))
        {
            let id = p.id().to_bytes();
            let hex = hex(&id);
            let app = apps.and_then(|apps| apps.iter().find(|a| a["project"] == hex));
            let runtime = match app.and_then(|a| a["observed"].as_str()) {
                Some("healthy") => "Healthy",
                Some("failed") => "Unhealthy",
                Some("stopped") => "Stopped",
                _ => {
                    if projects.deployment.is_none() {
                        "Not configured"
                    } else {
                        "Not observed"
                    }
                }
            };
            let metrics = app.and_then(|a| validated(&a["telemetry"], at));
            let reporting = metrics.is_some();
            if let Some(m) = metrics {
                telemetry.push(m);
            }
            let mut build = "No saved build attempts";
            if !view.history_available {
                build = "Build history unavailable";
            } else if let Some(record) = records.iter().rev().find(|r| r.project() == id) {
                let (state, _) = projects::history::state(record.state());
                build = match state {
                    "Succeeded" => "Latest saved build succeeded",
                    "Failed" => "Latest saved build failed",
                    "Interrupted" => "Latest saved build interrupted",
                    "Running" => "Saved running checkpoint · not live status",
                    "Queued" => "Saved queued checkpoint · not live status",
                    _ => "Latest saved build cancelled",
                };
                let item = OverviewOutcome {
                    project: p.name().into(),
                    label: build.into(),
                    href: format!(
                        "/projects/{hex}/builds/attempts/{}",
                        self::hex(&record.id().to_bytes())
                    ),
                };
                if matches!(state, "Failed" | "Interrupted") {
                    view.attention.push(item);
                } else {
                    view.outcomes.push(item);
                }
            }
            if runtime == "Unhealthy" {
                view.attention.push(OverviewOutcome {
                    project: p.name().into(),
                    label: "Runtime health check failed".into(),
                    href: format!("/projects/{hex}/releases"),
                });
            }
            view.projects.push(OverviewProject {
                id,
                name: p.name().into(),
                runtime,
                build,
                reporting,
            });
        }
        view.monitoring = aggregate(&telemetry, view.projects.len(), hours, at);
        view
    })
    .await
    .unwrap_or_else(|_| Overview {
        range_hours: hours,
        ..Default::default()
    })
}
fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Clone)]
struct Bucket {
    at: u64,
    requests: u64,
    errors: u64,
    active: u64,
    latency: [u64; 12],
}
struct Metrics {
    started: u64,
    active: u64,
    overflow: bool,
    buckets: Vec<Bucket>,
}
fn validated(v: &Value, at: u64) -> Option<Metrics> {
    if v["version"] != 1 || v["updated"].as_u64().is_none_or(|t| t > at || at - t > 45) {
        return None;
    }
    let started = v["started"].as_u64()?;
    if started == 0 || started > v["updated"].as_u64()? {
        return None;
    }
    let active = v["active"].as_u64()?;
    if active > 10000 {
        return None;
    }
    let raw = v["buckets"].as_array()?;
    if raw.len() > 289 {
        return None;
    }
    let mut buckets = Vec::new();
    let mut previous = None;
    for b in raw {
        let time = b["at"].as_u64()?;
        let requests = b["requests"].as_u64()?;
        let errors = b["errors"].as_u64()?;
        let active = b["active"].as_u64()?;
        if time % 300 != 0
            || time > at
            || time < at.saturating_sub(86700)
            || previous.is_some_and(|p| p >= time)
            || requests > 1_000_000_000
            || errors > requests
            || active > 10000
        {
            return None;
        }
        previous = Some(time);
        let h = b["latency"].as_array()?;
        if h.len() != 12 {
            return None;
        }
        let mut latency = [0; 12];
        let mut sum = 0u64;
        for (i, n) in h.iter().enumerate() {
            latency[i] = n.as_u64()?;
            sum = sum.checked_add(latency[i])?;
        }
        if sum != requests {
            return None;
        }
        buckets.push(Bucket {
            at: time,
            requests,
            errors,
            active,
            latency,
        });
    }
    Some(Metrics {
        started,
        active,
        overflow: v["overflow"].as_bool()?,
        buckets,
    })
}
fn percentile(h: &[u64; 12]) -> Option<u64> {
    let count: u64 = h.iter().sum();
    if count == 0 {
        return None;
    }
    let target = (count * 95).div_ceil(100);
    let mut sum = 0;
    for (i, n) in h.iter().enumerate() {
        sum += n;
        if sum >= target {
            return if i == 11 { None } else { Some(LATENCY[i]) };
        }
    }
    None
}
fn aggregate(metrics: &[Metrics], expected: usize, hours: u64, at: u64) -> Monitoring {
    let mut m = Monitoring {
        reporting: metrics.len(),
        expected,
        ..Default::default()
    };
    let end = at / 300 * 300;
    let start = end.saturating_sub(hours * 3600 - 300);
    let mut hist = [0u64; 12];
    let mut requests = 0;
    let mut errors = 0;
    if !metrics.is_empty() {
        m.active = Some(metrics.iter().map(|s| s.active).sum());
        m.overflow = metrics.iter().any(|s| s.overflow);
        m.since = metrics.iter().map(|s| s.started).max();
    }
    // Window totals include every available observation, even if a different
    // app restarted mid-window. Chart gaps still expose incomplete intervals.
    for bucket in metrics
        .iter()
        .flat_map(|m| m.buckets.iter())
        .filter(|b| b.at >= start && b.at <= end)
    {
        requests += bucket.requests;
        errors += bucket.errors;
        for (i, v) in bucket.latency.iter().enumerate() {
            hist[i] += v;
        }
    }
    for time in (start..=end).step_by(300) {
        let buckets: Vec<_> = metrics
            .iter()
            .filter_map(|s| s.buckets.iter().find(|b| b.at == time))
            .collect();
        let mut p = MetricPoint {
            at: time,
            active: None,
            requests: None,
            p95: None,
            errors: None,
        };
        // Plot only intervals covered by every currently reporting app. Missing
        // intervals stay gaps; a zero is only shown for an observed idle bucket.
        if !buckets.is_empty() && buckets.len() == metrics.len() {
            let n: u64 = buckets.iter().map(|b| b.requests).sum();
            let e: u64 = buckets.iter().map(|b| b.errors).sum();
            let mut h = [0u64; 12];
            for b in buckets.iter() {
                for (i, v) in b.latency.iter().enumerate() {
                    h[i] += v;
                }
            }

            p.active = Some(buckets.iter().map(|b| b.active as f64).sum());
            p.requests = Some(n as f64);
            p.p95 = percentile(&h).map(|n| n as f64);
            p.errors = if n > 0 {
                Some(e as f64 / n as f64 * 100.0)
            } else {
                None
            };
        }
        m.points.push(p);
    }
    if !metrics.is_empty() {
        m.requests = Some(requests);
        m.errors = if requests > 0 {
            Some(errors as f64 / requests as f64 * 100.0)
        } else {
            None
        };
        m.p95 = percentile(&hist);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metrics(at: u64, requests: u64, errors: u64, active: u64) -> Value {
        serde_json::json!({"version":1,"updated":at,"started":at-100,"active":active,"overflow":false,"buckets":[{"at":at/300*300,"requests":requests,"errors":errors,"active":active,"latency":[0,0,0,requests,0,0,0,0,0,0,0,0]}]})
    }
    #[test]
    fn stale_future_and_inconsistent_telemetry_is_not_healthy_coverage() {
        let v = metrics(1000, 10, 1, 4);
        assert!(validated(&v, 1045).is_some());
        assert!(validated(&v, 1046).is_none());
        assert!(validated(&v, 999).is_none());
        let mut bad = v.clone();
        bad["buckets"][0]["errors"] = 11.into();
        assert!(validated(&bad, 1000).is_none());
        bad = v.clone();
        bad["buckets"][0]["latency"][3] = 9.into();
        assert!(validated(&bad, 1000).is_none());
        bad = v.clone();
        bad["active"] = 10001.into();
        assert!(validated(&bad, 1000).is_none());
        bad = v;
        let bucket = bad["buckets"][0].clone();
        bad["buckets"].as_array_mut().unwrap().push(bucket);
        assert!(validated(&bad, 1000).is_none());
    }
    #[test]
    fn aggregates_histograms_and_rates_by_request_count_not_average_of_apps() {
        let a = validated(&metrics(1000, 100, 1, 3), 1000).unwrap();
        let mut b = metrics(1000, 10, 2, 5);
        b["buckets"][0]["latency"][3] = 0.into();
        b["buckets"][0]["latency"][4] = 10.into();
        let b = validated(&b, 1000).unwrap();
        let m = aggregate(&[a, b], 3, 1, 1000);
        assert_eq!(m.reporting, 2);
        assert_eq!(m.expected, 3);
        assert_eq!(m.active, Some(8));
        assert_eq!(m.requests, Some(110));
        assert!((m.errors.unwrap() - 3.0 / 110.0 * 100.0).abs() < 0.00001);
        assert_eq!(m.p95, Some(200));
        assert!(m.points[0].requests.is_none());
        assert_eq!(m.points.last().unwrap().requests, Some(110.0));
    }
    #[test]
    fn idle_has_zero_requests_but_no_invented_latency_or_error_rate() {
        let m = aggregate(
            &[validated(&metrics(1000, 0, 0, 0), 1000).unwrap()],
            1,
            24,
            1000,
        );
        assert_eq!(m.active, Some(0));
        assert_eq!(m.requests, Some(0));
        assert_eq!(m.p95, None);
        assert_eq!(m.errors, None);
        let missing = aggregate(&[], 2, 24, 1000);
        assert_eq!(missing.active, None);
        assert_eq!(missing.requests, None);
    }
}
