use std::process::Command;

#[test]
fn version_prints_semver_only() {
    let out = Command::new(env!("CARGO_BIN_EXE_apmux"))
        .arg("--version")
        .output()
        .expect("run apmux --version");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn update_help_lists_check_and_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_apmux"))
        .args(["update", "--help"])
        .output()
        .expect("run apmux update --help");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--check"), "{stdout}");
    assert!(stdout.contains("--version"), "{stdout}");
    assert!(stdout.contains("--json"), "{stdout}");
}
