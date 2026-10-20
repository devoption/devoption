//! Owner guidance only: this view never grants service-control authority.
use crate::projects::escape_into;

#[derive(Default)]
pub struct PlatformUpdates {
    pub running_version: String,
    pub unavailable: bool,
    pub notice: Option<UpgradeNotice>,
}

pub struct UpgradeNotice {
    pub previous_version: String,
    pub candidate_version: String,
    pub archive_sha256: String,
    pub source_commit: String,
    pub transaction: String,
    pub attempt: u64,
    pub phase: String,
    pub age_seconds: u64,
}

pub(super) fn content(view: &PlatformUpdates) -> String {
    let mut html = String::from(
        r#"<section class="platform-update" aria-labelledby="platform-update-title"><div class="section-heading"><div><h2 id="platform-update-title">DevOption updates</h2><p>Running version <strong>"#,
    );
    escape_into(&mut html, &view.running_version);
    html.push_str(
        r#"</strong></p></div><a class="action-button" href="/">Refresh status</a></div>"#,
    );
    match &view.notice {
        None => html.push_str(if view.unavailable {
            "<p>Update status is unavailable. Ask the server administrator to inspect the upgrade notice and run the verified status command over SSH.</p>"
        } else {
            "<p>No reviewed update has been checked on this server. The server administrator can check a selected candidate over SSH.</p>"
        }),
        Some(notice) => {
            let (heading, message) = if notice.age_seconds > 900 {
                ("Update status needs a fresh check", "The saved observation is too old to guide an upgrade. Run a verified plan or status check over SSH.")
            } else { match notice.phase.as_str() {
                "not-started" if view.running_version == notice.previous_version => (
                    "Preview update available", "The server administrator selected this candidate. Arrange a maintenance window and review a fresh plan over SSH before applying it."),
                "healthy" if view.running_version == notice.candidate_version => (
                    "Upgrade completed", "The recorded upgrade passed readiness checks. Check owner login and saved data before ending the maintenance window."),
                "restored" if view.running_version == notice.previous_version => (
                    "Previous version restored", "The recorded full-state recovery completed. Check owner login and saved data before ending the maintenance window."),
                "failed" => ("Upgrade needs attention", "Preserve the transaction and backup. Inspect status over SSH before retrying or recovering."),
                "prepared" | "backed-up" | "activated" | "restoring" => ("Upgrade is unfinished", "Inspect the recorded transaction over SSH. Recovery is explicit and may discard post-backup writes."),
                _ => ("Check installed upgrade status", "The running version differs from the recorded result. Inspect the installed service and transaction over SSH before proceeding."),
            }};
            html.push_str("<h3>"); html.push_str(heading); html.push_str("</h3><p>"); html.push_str(message); html.push_str("</p><p>Selected preview: <strong>");
            escape_into(&mut html, &notice.candidate_version);
            html.push_str("</strong>. This validation candidate is unpublished and is not eligible for product publication.</p>");
            html.push_str(&format!("<p class=\"muted\">Recorded {} seconds ago. This is a saved observation, not continuous health monitoring.</p>", notice.age_seconds));
            if notice.age_seconds > 900 {
                html.push_str("<p><strong>This observation is stale.</strong> Run a new verified plan or status check over SSH before making a decision.</p>");
            }
            html.push_str("<details><summary>Candidate and transaction details</summary><dl><dt>Previous version</dt><dd>");
            escape_into(&mut html, &notice.previous_version);
            for (label, value) in [("Archive SHA-256", &notice.archive_sha256), ("Source commit", &notice.source_commit), ("Transaction", &notice.transaction), ("Recorded phase", &notice.phase)] {
                html.push_str("</dd><dt>"); html.push_str(label); html.push_str("</dt><dd><code>"); escape_into(&mut html, value); html.push_str("</code>");
            }
            html.push_str(&format!("</dd><dt>Attempt</dt><dd>{}</dd></dl></details>", notice.attempt));
        }
    }
    html.push_str(r#"<details class="platform-update-steps"><summary>Review upgrade and recovery steps</summary><ol><li>Connect to the server over SSH using your administrator account.</li><li>In the reviewed trusted checkout, select the exact previous and candidate reservations, signed packets and pinned verifier/proxy inputs described in <code>docs/platform-upgrades.md</code>.</li><li>Run <code>sudo python3 scripts/upgrade_bundle.py plan</code> with those reviewed arguments. Recheck the candidate identity, supported path and backup capacity.</li><li>During an agreed maintenance window, run <code>apply</code> with the same arguments, then <code>status</code>. Verify login, saved data and reboot readiness.</li><li>If interrupted or failed, preserve the transaction and backup. Inspect <code>status</code> before explicit <code>recover</code>. Recovery causes downtime and can remove post-backup writes.</li></ol><p>Commands require the full verified arguments from the guide. The browser does not download candidates, run privileged commands or start unattended upgrades. App/build installations and schema-changing migrations remain unsupported.</p></details></section>"#);
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(phase: &str) -> PlatformUpdates {
        PlatformUpdates {
            running_version: "0.1.0-preview.1".into(),
            unavailable: false,
            notice: Some(UpgradeNotice {
                previous_version: "0.1.0-preview.1".into(),
                candidate_version: "0.1.0-preview.2".into(),
                archive_sha256: "a".repeat(64),
                source_commit: "b".repeat(40),
                transaction: "c".repeat(64),
                attempt: 2,
                phase: phase.into(),
                age_seconds: 42,
            }),
        }
    }

    #[test]
    fn guidance_distinguishes_available_completed_recovered_failed_and_mismatched_versions() {
        for (phase, running, heading) in [
            ("not-started", "0.1.0-preview.1", "Preview update available"),
            ("healthy", "0.1.0-preview.2", "Upgrade completed"),
            ("restored", "0.1.0-preview.1", "Previous version restored"),
            ("failed", "0.1.0-preview.1", "Upgrade needs attention"),
            ("backed-up", "0.1.0-preview.2", "Upgrade is unfinished"),
            (
                "healthy",
                "0.1.0-preview.1",
                "Check installed upgrade status",
            ),
        ] {
            let mut state = view(phase);
            state.running_version = running.into();
            let html = content(&state);
            assert!(html.contains(heading));
            assert!(html.contains("SSH"));
            assert!(html.contains("post-backup writes"));
            assert!(!html.contains("<form"));
            assert!(!html.contains("<script"));
        }
    }

    #[test]
    fn stale_and_unavailable_data_do_not_claim_current_readiness_and_values_are_escaped() {
        let mut state = view("healthy");
        state.notice.as_mut().unwrap().age_seconds = 901;
        state.running_version = "<img src=x onerror=bad()>".into();
        let html = content(&state);
        assert!(html.contains("observation is stale"));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;img"));
        state.notice = None;
        state.unavailable = true;
        assert!(content(&state).contains("status is unavailable"));
        state.unavailable = false;
        assert!(content(&state).contains("No reviewed update"));
    }
}
