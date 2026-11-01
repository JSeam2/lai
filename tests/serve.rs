use std::{fs, process::Command};

#[test]
fn serve_help_documents_detached_mode_and_port() {
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["serve", "--help"])
        .output()
        .expect("CLI should start");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(
        help.contains("--detached") && help.contains("--port"),
        "{help}"
    );
}

#[test]
fn serve_requires_a_configured_model() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["serve"])
        .env("HOME", home.path())
        .output()
        .expect("CLI should start");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("lai config model set"));
}

#[test]
fn serve_reports_missing_download() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let directory = home.path().join(".lai");
    fs::create_dir(&directory).expect("config directory should be created");
    fs::write(directory.join("config.toml"), "model = \"owner/model\"\n")
        .expect("config should be written");
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["serve", "-d"])
        .env("HOME", home.path())
        .output()
        .expect("CLI should start");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("lai download owner/model"));
}
