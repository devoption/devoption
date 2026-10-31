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
    write!(html, "<section class=\"overview-status\" aria-label=\"Operational summary\"><div><h2>{title}</h2><p>{healthy} of {} projects verified healthy. Security scanning is not connected.</p></div><div><strong>{attention}</strong><p>Based on saved builds and runtime checks.</p></div></section>",view.projects.len()).unwrap();
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
    html.push_str("</section><aside><section class=\"overview-panel\" aria-labelledby=\"overview-attention\"><h2 id=\"overview-attention\">Needs attention</h2>");
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
    html.push_str("</section><section class=\"overview-panel\" aria-labelledby=\"overview-outcomes\"><h2 id=\"overview-outcomes\">Recorded outcomes</h2><p class=\"muted\">Latest saved build results.</p>");
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
fn count(value: u64) -> String {
    let digits = value.to_string();
    digits
        .chars()
        .enumerate()
        .fold(String::new(), |mut out, (i, c)| {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
            out
        })
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
    html.push_str("</nav></div><div class=\"overview-observatory\">");
    let values = [
        m.active
            .map(|v| format!("{}{}", if m.overflow { "≥" } else { "" }, count(v)))
            .unwrap_or_else(|| "—".into()),
        m.requests.map(count).unwrap_or_else(|| "—".into()),
        m.p95
            .map(|v| format!("{v} ms"))
            .unwrap_or_else(|| "—".into()),
        m.errors
            .map(|v| format!("{v:.2}%"))
            .unwrap_or_else(|| "—".into()),
    ];
    let labels = [
        "Active users",
        "Requests",
        "Response time · p95",
        "Error rate",
    ];
    let notes = [
        "Estimated browsers · last 5 min",
        "Requests per 5 minutes",
        "Handling time · histogram estimate",
        "HTTP 5xx / observed requests",
    ];
    let units = ["browsers", "requests", "ms", "%"];
    let series: Vec<Vec<Option<f64>>> = (0..4)
        .map(|i| {
            m.points
                .iter()
                .map(|p| match i {
                    0 => p.active,
                    1 => p.requests,
                    2 => p.p95,
                    _ => p.errors,
                })
                .collect()
        })
        .collect();
    html.push_str("<fieldset class=\"overview-metrics\"><legend class=\"visually-hidden\">Metric to chart</legend>");
    let summaries = [
        "Browsers · last 5 min",
        "Observed in this window",
        "Histogram estimate",
        "HTTP 5xx responses",
    ];
    for i in [1, 0, 2, 3] {
        write!(html,"<label class=\"overview-metric-choice\"><input class=\"visually-hidden\" type=\"radio\" name=\"overview-metric\" value=\"{i}\" {}><span id=\"overview-metric-{i}-label\" class=\"overview-metric-label\">{}</span><span class=\"overview-number\">{}</span><span class=\"overview-chart-caption\">{}</span></label>",if i==1 {"checked"} else {""},labels[i],values[i],summaries[i]).unwrap();
    }
    html.push_str("</fieldset><div class=\"overview-trends\">");
    for i in [1, 0, 2, 3] {
        write!(html,"<section class=\"overview-chart\" data-metric=\"{i}\" aria-labelledby=\"overview-metric-{i}-label\"><p class=\"overview-chart-caption\">{}</p>{}</section>",notes[i],chart(&series[i],labels[i],units[i],hours,false)).unwrap();
    }
    write!(html,"</div></div><div class=\"overview-monitoring-footer\"><p>{} of {} projects reporting · Missing intervals remain gaps</p><details><summary>Chart data</summary><p>Five-minute buckets, oldest first. A dash means missing observations.</p>",m.reporting,m.expected).unwrap();
    for i in [1, 0, 2, 3] {
        write!(
            html,
            "<h4>{} ({})</h4><p class=\"overview-data\">{}</p>",
            labels[i],
            units[i],
            series[i]
                .iter()
                .map(|v| v.map(|n| format!("{n:.2}")).unwrap_or_else(|| "—".into()))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
    html.push_str("</details></div><details class=\"overview-metric-notes\"><summary>About these metrics</summary><p>Five-minute buckets, retained for up to 24 hours. History resets on worker restart. Latency uses histogram upper bounds; errors are HTTP 5xx responses. Health probes are excluded.</p>");
    if m.since.is_some() {
        html.push_str("<p>Values cover available observations only. Active users are browser estimates summed per app; the same person can count in multiple apps. Cookie rejection can inflate estimates; non-browser clients are not identified.</p>");
    }
    html.push_str("</details></section>");
    html
}
fn axis(value: f64, unit: &str) -> String {
    if unit == "%" {
        format!("{value:.1}%")
    } else if value >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value:.0}")
    }
}
fn chart_ceiling(value: f64) -> f64 {
    let magnitude = 10_f64.powf(value.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .map(|step| step * magnitude)
        .find(|step| *step >= value)
        .unwrap_or(10.0 * magnitude)
}
fn chart(series: &[Option<f64>], label: &str, unit: &str, hours: u64, compact: bool) -> String {
    let floor = 1.0;
    let observed = series.iter().flatten().copied().fold(floor, f64::max);
    let max = if unit == "%" && observed <= 1.0 {
        1.0
    } else {
        chart_ceiling(observed * 1.15)
    };
    let mut line = String::new();
    let mut area = String::new();
    let mut dots = String::new();
    let mut open = false;
    let mut last_x = 0.0;
    for (i, value) in series.iter().enumerate() {
        if let Some(value) = value {
            let x = i as f64 / series.len().saturating_sub(1).max(1) as f64 * 1000.0;
            let y = 200.0 - value / max * 200.0;
            write!(line, "{} {x:.1} {y:.1} ", if open { "L" } else { "M" }).unwrap();
            if !open {
                write!(area, "M {x:.1} 200 L {x:.1} {y:.1} ").unwrap();
            } else {
                write!(area, "L {x:.1} {y:.1} ").unwrap();
            }
            if (i == 0 || series[i - 1].is_none())
                && (i + 1 == series.len() || series[i + 1].is_none())
            {
                // A short line segment has a stable visible width even when the
                // responsive plot stretches. Do not bridge missing samples.
                write!(dots, "M {:.1} {y:.1} L {:.1} {y:.1} ", x - 1.0, x + 1.0).unwrap();
            }
            open = true;
            last_x = x;
        } else if open {
            write!(area, "L {last_x:.1} 200 Z ").unwrap();
            open = false;
        }
    }
    if open {
        write!(area, "L {last_x:.1} 200 Z").unwrap();
    }
    let empty = if line.is_empty() {
        "<span class=\"overview-no-data\">No observations</span>"
    } else {
        ""
    };
    let mut html = format!(
        "<div class=\"overview-plot{}\"><div class=\"overview-y-axis\" aria-hidden=\"true\"><span>{}</span><span>{}</span><span>{}</span></div><div class=\"overview-plot-canvas\"><svg viewBox=\"0 0 1000 200\" preserveAspectRatio=\"none\" role=\"img\" aria-label=\"{}; {}; last {hours} hours. Missing intervals are gaps.\"><path class=\"overview-grid\" d=\"M0 200H1000\"/><path class=\"overview-area\" d=\"{area}\"/><path class=\"overview-line\" vector-effect=\"non-scaling-stroke\" d=\"{line}{dots}\"/></svg>{empty}</div>",
        if compact {
            " overview-plot-compact"
        } else {
            ""
        },
        if line.is_empty() {
            String::new()
        } else {
            axis(max, unit)
        },
        if line.is_empty() {
            String::new()
        } else {
            axis(max / 2.0, unit)
        },
        if line.is_empty() {
            ""
        } else if unit == "%" {
            "0%"
        } else {
            "0"
        },
        escaped(label),
        escaped(unit)
    );
    write!(html,"<div class=\"overview-x-axis\" aria-hidden=\"true\"><span>−{hours}h</span><span>−{}h</span><span>Now</span></div></div>",hours as f64/2.0).unwrap();
    html
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
        assert!(!h.contains("Agent operations"));
        assert!(!h.contains("Everything is healthy"));
        assert!(h.contains("/monitoring"));
    }
    #[test]
    fn monitoring_keeps_units_and_precision_without_inventing_empty_axis_values() {
        assert_eq!(count(1234567), "1,234,567");
        let empty = chart(&[None, None], "Errors", "%", 24, false);
        assert!(!empty.contains("1.0%"));
        assert!(empty.contains("No observations"));
        let observed = chart(&[Some(0.08), Some(0.1)], "Errors", "%", 24, true);
        assert!(observed.contains("1.0%"));
        assert!(observed.contains("0.5%"));
        assert!(observed.contains("vector-effect=\"non-scaling-stroke\""));
    }
    #[test]
    fn chart_breaks_lines_for_missing_intervals() {
        let h = chart(
            &[Some(0.0), None, Some(2.0)],
            "Traffic",
            "requests",
            1,
            false,
        );
        assert!(h.contains("M 0.0 200.0 M 1000.0"));
        assert!(h.contains("preserveAspectRatio=\"none\""));
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
