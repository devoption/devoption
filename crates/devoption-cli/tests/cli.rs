use std::process::Command;

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
        assert!(text.contains("devoption setup --data-dir ABSOLUTE_PATH"));
        assert!(text.contains("Serve creates no accounts; setup never replaces an owner"));
        assert!(text.contains("After CLI setup, run devoption serve --data-dir ABSOLUTE_PATH"));
        assert!(text.contains("with the same directory, then sign in at /sign-in"));
        assert!(text.contains("Browser owner setup is not available"));
        assert!(text.contains("services are not implemented"));
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
        vec!["serve", "--port"],
        vec!["serve", "--port", "abc"],
        vec!["serve", "--port", "65536"],
        vec!["serve", "--port", "-1"],
        vec!["serve", "--host", "0.0.0.0"],
        vec!["serve", "--port", "0", "extra"],
        vec!["serve", "--port", "0", "--port", "0"],
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
