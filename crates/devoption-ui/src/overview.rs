//! Evidence-only operational summaries, rendered with the existing app theme.
use crate::projects::escape_into;
use std::fmt::Write;

#[derive(Default)]
pub struct Overview {
    pub projects: Vec<OverviewProject>,
    pub catalog_available: bool,
    pub history_available: bool,
    pub deployment_available: bool,
    pub monitoring: Monitoring,
    pub outcomes: Vec<OverviewOutcome>,
    pub attention: Vec<OverviewOutcome>,
    pub range_hours: u64,
}
pub struct OverviewProject {
    pub id: [u8; 16],
    pub name: String,
    pub runtime: &'static str,
    pub build: &'static str,
    pub reporting: bool,
}
pub struct OverviewOutcome {
    pub project: String,
    pub label: String,
    pub href: String,
}
#[derive(Default, Clone)]
pub struct Monitoring {
    pub reporting: usize,
    pub expected: usize,
    pub active: Option<u64>,
    pub requests: Option<u64>,
    pub errors: Option<f64>,
    pub p95: Option<u64>,
    pub overflow: bool,
    pub since: Option<u64>,
    pub points: Vec<MetricPoint>,
}
#[derive(Clone)]
pub struct MetricPoint {
    pub at: u64,
    pub active: Option<f64>,
    pub requests: Option<f64>,
    pub p95: Option<f64>,
    pub errors: Option<f64>,
}
pub(crate) fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}
fn escaped(text: &str) -> String {
    let mut h = String::new();
    escape_into(&mut h, text);
    h
}
pub(super) fn content(view: &Overview) -> String {
    let mut html = String::from("<div class=\"operational-overview\">");
    let failures = view
        .projects
        .iter()
        .filter(|p| p.runtime == "Unhealthy")
        .count();
    let healthy = view
        .projects
        .iter()
        .filter(|p| p.runtime == "Healthy")
        .count();
    let title = if !view.catalog_available {
        "Workspace status unavailable"
    } else if failures > 0 {
        "Applications need attention"
    } else if healthy > 0 {
        "Applications healthy · coverage incomplete"
    } else {
        "Health coverage incomplete"
    };
    let attention = if !view.attention.is_empty() {
        format!("{} recorded items to review", view.attention.len())
    } else if view.history_available {
        "No recorded issues".into()
    } else {
        "Attention coverage incomplete".into()
    };
    write!(html, "<section class=\"overview-status\" aria-label=\"Operational summary\"><div><h2>{title}</h2><p>{healthy} of {} projects verified healthy. Security and agents are not connected.</p></div><div><strong>{attention}</strong><p>Agent approvals are not tracked yet.</p></div></section>",view.projects.len()).unwrap();
    html.push_str(&monitoring(&view.monitoring, view.range_hours, "/"));
    html.push_str("<div class=\"overview-columns\"><section aria-labelledby=\"overview-projects\"><div class=\"section-heading\"><h2 id=\"overview-projects\">Projects</h2><a href=\"/projects\">View all →</a></div>");
    if !view.catalog_available {
        html.push_str("<p role=\"status\">Projects could not be loaded. Refresh to try again.</p>");
    } else if view.projects.is_empty() {
        html.push_str("<div class=\"overview-project\"><h3>No projects yet</h3><p>Create a project to start collecting operational evidence.</p><a class=\"action-button primary\" href=\"/projects\">Open Projects →</a></div>");
    }
    for p in view.projects.iter().take(8) {
        let id = hex(&p.id);
        write!(html,"<article class=\"overview-project\"><div class=\"section-heading\"><h3><a href=\"/projects/{id}\">{}</a></h3><a class=\"overview-state\" href=\"/projects/{id}/releases\">{}</a></div><ul class=\"overview-signals\"><li>Security: not connected</li><li>Telemetry: {}</li></ul><div class=\"overview-work\"><a href=\"/projects/{id}/builds\">{}</a><a href=\"/projects/{id}/monitoring\">Monitoring →</a></div></article>",escaped(&p.name),p.runtime,if p.reporting { "reporting" } else { "unavailable" },p.build).unwrap();
    }
    if view.projects.len() > 8 {
        html.push_str(
            "<p>Showing the first 8 projects. Open Projects to see all saved projects.</p>",
        );
    }
    html.push_str("</section><aside><section class=\"overview-panel\" aria-labelledby=\"overview-agents\"><h2 id=\"overview-agents\">Agent operations</h2><p class=\"overview-number\">Not connected</p><p>Connect an agent execution service to see progress and decisions here.</p></section><section class=\"overview-panel\" aria-labelledby=\"overview-attention\"><h2 id=\"overview-attention\">Needs attention</h2>");
    if !view.history_available {
        html.push_str(
            "<p>Build history unavailable. Failed or interrupted builds cannot be checked.</p>",
        );
    }
    list(
        &mut html,
        &view.attention,
        "No issues found in the available build and runtime observations.",
    );
    html.push_str("</section><section class=\"overview-panel\" aria-labelledby=\"overview-outcomes\"><h2 id=\"overview-outcomes\">Recorded outcomes</h2><p class=\"muted\">Saved build results; not a live agent activity feed.</p>");
    list(&mut html, &view.outcomes, "No saved build outcomes.");
    html.push_str("</section></aside></div><p class=\"overview-footer\">Runtime checks are point-in-time observations. <a href=\"/\">Refresh Overview</a></p></div>");
    html
}
fn list(html: &mut String, items: &[OverviewOutcome], empty: &str) {
    if items.is_empty() {
        write!(html, "<p>{empty}</p>").unwrap();
        return;
    }
    html.push_str("<ul class=\"overview-outcomes\">");
    for item in items.iter().take(6) {
        write!(
            html,
            "<li><a href=\"{}\">{}</a><span>{}</span></li>",
            escaped(&item.href),
            escaped(&item.project),
            escaped(&item.label)
        )
        .unwrap();
    }
    html.push_str("</ul>");
    if items.len() > 6 {
        html.push_str(
            "<p>More items are available in each project's build and release history.</p>",
        );
    }
}
pub(super) fn monitoring(m: &Monitoring, hours: u64, base: &str) -> String {
    let hours = if hours == 1 { 1 } else { 24 };
    let mut html = String::from(
        "<section class=\"overview-monitoring\" aria-labelledby=\"overview-monitoring-title\"><div class=\"section-heading\"><h2 id=\"overview-monitoring-title\">Monitoring</h2><nav class=\"overview-ranges\" aria-label=\"Monitoring time range\">",
    );
    for h in [1, 24] {
        write!(
            html,
            "<a href=\"{base}?range={h}h\" {}>{h}h</a>",
            if h == hours {
                "aria-current=\"true\""
            } else {
                ""
            }
        )
        .unwrap();
    }
    html.push_str("</nav></div><div class=\"overview-charts\">");
    let active = m
        .active
        .map(|v| format!("{}{v}", if m.overflow { "≥" } else { "" }))
        .unwrap_or_else(|| "—".into());
    let values = [
        active,
        m.requests
            .map(|v| v.to_string())
            .unwrap_or_else(|| "—".into()),
        m.p95
            .map(|v| format!("{v} ms"))
            .unwrap_or_else(|| "—".into()),
        m.errors
            .map(|v| format!("{v:.2}%"))
            .unwrap_or_else(|| "—".into()),
    ];
    for (index, (title, note)) in [
        ("Active users", "Estimated browsers · last 5 min"),
        ("Requests", "Observed requests in this window"),
        (
            "Response time · p95",
            "Histogram estimate · request handling",
        ),
        ("Error rate", "HTTP 5xx · observed requests"),
    ]
    .iter()
    .enumerate()
    {
        write!(html,"<article class=\"overview-chart\"><h3>{title}</h3><p class=\"overview-number\">{}</p><p class=\"muted\">{note}</p>",values[index]).unwrap();
        let series: Vec<_> = m
            .points
            .iter()
            .map(|p| match index {
                0 => p.active,
                1 => p.requests,
                2 => p.p95,
                _ => p.errors,
            })
            .collect();
        html.push_str(&chart(&series, title, 1.0, hours));
        html.push_str("</article>");
    }
    write!(html,"</div><p class=\"muted\">{} of {} projects reporting. Gaps mean unavailable data, not zero traffic. </p>",m.reporting,m.expected).unwrap();
    html.push_str("<details class=\"overview-metric-notes\"><summary>About these metrics</summary><p>Five-minute buckets, retained for up to 24 hours. History resets on worker restart. Latency uses histogram upper bounds; errors are HTTP 5xx responses. Health probes are excluded.</p>");
    if m.since.is_some() {
        html.push_str("<p class=\"muted\">Values cover available observations only. Active users are browser estimates summed per app; the same person can count in multiple apps. Cookie rejection can inflate estimates; non-browser clients are not identified.</p>");
    }
    html.push_str("</details></section>");
    html
}
fn chart(series: &[Option<f64>], label: &str, floor: f64, hours: u64) -> String {
    let max = series.iter().flatten().copied().fold(floor, f64::max);
    let mut path = String::new();
    let mut open = false;
    for (i, v) in series.iter().enumerate() {
        if let Some(v) = v {
            let x = 30.0 + i as f64 / (series.len().saturating_sub(1).max(1)) as f64 * 260.0;
            let y = 90.0 - v / max * 70.0;
            write!(path, "{} {x:.1} {y:.1} ", if open { "L" } else { "M" }).unwrap();
            open = true;
        } else {
            open = false;
        }
    }
    let mut dots = String::new();
    for (i, v) in series.iter().enumerate() {
        if let Some(v) = v
            && (i == 0 || series[i - 1].is_none())
            && (i + 1 == series.len() || series[i + 1].is_none())
        {
            let x = 30.0 + i as f64 / series.len().saturating_sub(1).max(1) as f64 * 260.0;
            let y = 90.0 - v / max * 70.0;
            write!(
                dots,
                "<circle class=\"overview-point\" cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"2.5\"/>"
            )
            .unwrap();
        }
    }
    let empty = if path.is_empty() {
        "<text x=\"160\" y=\"57\" text-anchor=\"middle\">No observations</text>"
    } else {
        ""
    };
    format!(
        "<svg viewBox=\"0 0 320 120\" role=\"img\" aria-label=\"{}; last {hours} hours. Missing intervals are gaps.\"><path class=\"overview-grid\" d=\"M30 20H295 M30 55H295 M30 90H295\"/><text x=\"2\" y=\"24\">{max:.0}</text><text x=\"10\" y=\"94\">0</text><path class=\"overview-line\" d=\"{path}\"/>{dots}{empty}<text x=\"30\" y=\"115\">−{hours}h</text><text x=\"270\" y=\"115\">Now</text></svg><details><summary>Chart data</summary><p>Five-minute buckets, oldest first. A dash means missing observations.</p><p class=\"overview-data\">{}</p></details>",
        escaped(label),
        series
            .iter()
            .map(|v| v.map(|n| format!("{n:.2}")).unwrap_or_else(|| "—".into()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_project_names_and_has_no_fabricated_good_state() {
        let v = Overview {
            catalog_available: true,
            projects: vec![OverviewProject {
                id: [1; 16],
                name: "<script>private</script>".into(),
                runtime: "Not observed",
                build: "Build history unavailable",
                reporting: false,
            }],
            ..Default::default()
        };
        let h = content(&v);
        assert!(h.contains("&lt;script&gt;"));
        assert!(!h.contains("<script>private"));
        assert!(h.contains("No observations"));
        assert!(h.contains("Not connected"));
        assert!(!h.contains("Everything is healthy"));
        assert!(h.contains("/monitoring"));
    }
    #[test]
    fn chart_breaks_lines_for_missing_intervals() {
        let h = chart(&[Some(0.0), None, Some(2.0)], "Traffic", 1.0, 1);
        assert!(h.contains("M 30.0 90.0 M 290.0 20.0"));
        assert!(h.contains("Chart data"));
    }
}

#[cfg(test)]
mod capture {
    use super::*;
    #[test]
    #[ignore = "explicit synthetic monitoring capture, not live runtime evidence"]
    fn manual_overview_monitoring_fixture() {
        let Some(path) = std::env::var_os("DEVOPTION_OVERVIEW_CAPTURE") else {
            return;
        };
        let mut view = Overview {
            catalog_available: true,
            history_available: true,
            range_hours: 24,
            ..Default::default()
        };
        for (index, name) in ["Example application", "Documentation"].iter().enumerate() {
            view.projects.push(OverviewProject {
                id: [index as u8 + 1; 16],
                name: (*name).into(),
                runtime: "Healthy",
                build: "Latest saved build succeeded",
                reporting: true,
            });
        }
        view.monitoring = Monitoring {
            reporting: 2,
            expected: 2,
            active: Some(128),
            requests: Some(84200),
            errors: Some(0.08),
            p95: Some(200),
            since: Some(1),
            ..Default::default()
        };
        for i in 0..288 {
            let wave = (i as f64 / 24.0).sin();
            view.monitoring.points.push(MetricPoint {
                at: i * 300,
                active: Some(110.0 + 25.0 * wave),
                requests: Some(290.0 + 80.0 * wave),
                p95: Some(200.0),
                errors: Some(0.08 + 0.02 * wave),
            });
        }
        view.monitoring.points.last_mut().unwrap().active = Some(128.0);
        view.monitoring.requests = Some(
            view.monitoring
                .points
                .iter()
                .map(|p| p.requests.unwrap().round() as u64)
                .sum(),
        );
        std::fs::write(
            path,
            crate::render_overview(
                &view,
                &crate::PlatformUpdates {
                    running_version: env!("CARGO_PKG_VERSION").into(),
                    ..Default::default()
                },
            )
            .replace(
                "Application health, ongoing work, and decisions that need you.",
                "Synthetic telemetry preview · No applications or agents were run.",
            ),
        )
        .unwrap();
    }
}
