use std::process::Command;

#[test]
fn packaged_worker_identity_and_help_match_the_platform_version() {
    let binary = env!("CARGO_BIN_EXE_devoption-runtime");
    for (flag, expected) in [
        (
            "--version",
            format!("devoption-runtime {}\n", env!("CARGO_PKG_VERSION")),
        ),
        (
            "--help",
            "Usage: devoption-runtime app-worker COMPONENT PORT serve|validate\n".into(),
        ),
    ] {
        let output = Command::new(binary).env_clear().arg(flag).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, expected.as_bytes());
        assert!(output.stderr.is_empty());
    }
    let output = Command::new(binary)
        .args(["--version", "extra"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}
