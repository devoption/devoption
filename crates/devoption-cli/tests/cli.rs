use std::process::Command;

#[test]
fn repository_recovery_rejects_invalid_or_duplicate_options() {
    for args in [
        vec![],
        vec!["--data-dir", "relative", "--repository-root", "/missing"],
        vec![
            "--data-dir",
            "/missing",
            "--repository-root",
            "/missing",
            "--approve-device-rebind",
            "not-a-review",
        ],
        vec![
            "--data-dir",
            "/missing",
            "--data-dir",
            "/other",
            "--repository-root",
            "/missing",
        ],
        vec![
            "--data-dir",
            "/missing",
            "--repository-root",
            "/missing",
            "--force",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
            .arg("recover-repositories")
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn help_explains_the_actual_scope() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("Usage: devoption"));
        assert!(text.contains("--data-dir ABSOLUTE_PATH"));
        assert!(text.contains("--public-origin HTTPS_ORIGIN"));
        assert!(text.contains("--repository-root ABSOLUTE_PATH"));
        assert!(text.contains("--managed-repository-root ABSOLUTE_PATH"));
        assert!(text.contains("devoption setup --data-dir ABSOLUTE_PATH"));
        assert!(text.contains("Serve creates no accounts; setup never replaces an owner"));
        assert!(text.contains("After CLI setup, run devoption serve --data-dir ABSOLUTE_PATH"));
        assert!(text.contains("with the same directory, then sign in at /sign-in"));
        assert!(text.contains("Browser owner setup is not available"));
        assert!(text.contains("Full factory automation is not implemented"));
        assert!(text.contains("--app-deployment enabled"));
        assert!(text.contains("Installed Linux app deployment is opt-in"));
        assert!(text.contains("devoption check-source --data-dir"));
        assert!(text.contains("--project 32HEX --repository 32HEX"));
        assert!(
            text.contains("0 checks passed, 1 changes needed or unavailable, 2 invalid arguments")
        );
    }
}

#[test]
fn version_matches_the_built_package() {
    for flag in ["--version", "-V"] {
        let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(
            output.stdout,
            format!("devoption {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
        );
    }
}

#[test]
fn invalid_commands_fail_without_success_output() {
    for args in [
        vec!["start"],
        vec!["setup"],
        vec!["setup", "--data-dir"],
        vec!["setup", "--data-dir", "relative"],
        vec![
            "setup",
            "--data-dir",
            "/unused",
            "--password",
            "fixture only",
        ],
        vec!["setup", "--data-dir", "/unused", "--data-dir", "/unused"],
        vec!["--help", "start"],
        vec!["--version", "-V"],
        vec!["serve", "--public-origin"],
        vec!["serve", "--public-origin", "https://platform.example.test"],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--public-origin",
            "http://platform.example.test",
        ],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--public-origin",
            "https://platform.example.test/",
        ],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--public-origin",
            "https://platform.example.test",
            "--public-origin",
            "https://other.example.test",
        ],
        vec!["serve", "--port"],
        vec!["serve", "--port", "abc"],
        vec!["serve", "--port", "65536"],
        vec!["serve", "--port", "-1"],
        vec!["serve", "--host", "0.0.0.0"],
        vec!["serve", "--port", "0", "extra"],
        vec!["serve", "--port", "0", "--port", "0"],
        vec!["serve", "--managed-repository-root"],
        vec!["serve", "--managed-repository-root", "/unused"],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--managed-repository-root",
            "relative",
        ],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--managed-repository-root",
            "/unused",
            "--repository-root",
            "/unused",
        ],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--repository-root",
            "/unused",
            "--managed-repository-root",
            "/unused",
        ],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--managed-repository-root",
            "/unused",
            "--managed-repository-root",
            "/unused",
        ],
        vec!["serve", "--repository-root"],
        vec!["serve", "--repository-root", "/unused"],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--repository-root",
            "relative",
        ],
        vec![
            "serve",
            "--data-dir",
            "/unused",
            "--repository-root",
            "/unused",
            "--repository-root",
            "/unused",
        ],
        vec!["serve", "--data-dir"],
        vec!["serve", "--data-dir", ""],
        vec!["serve", "--data-dir", "relative"],
        vec!["serve", "--data-dir", "--port", "0"],
        vec!["serve", "--data-dir", "/unused", "--data-dir", "/unused"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("use --help")
        );
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_input_is_rejected_without_panicking() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let output = Command::new(env!("CARGO_BIN_EXE_devoption"))
        .arg(OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("panicked")
    );
}
