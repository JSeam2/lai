use std::{fs, path::Path, process::Command};

fn run(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lai"))
        .env("HOME", home)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn memory_configuration_and_management() -> anyhow::Result<()> {
    let home = tempfile::tempdir()?;
    for args in [
        vec!["config", "memory", "profile", "work.sqlite"],
        vec!["config", "memory", "enabled", "false"],
    ] {
        let out = run(home.path(), &args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let config = fs::read_to_string(home.path().join(".lai/config.toml"))?;
    assert!(config.contains("work.sqlite") && config.contains("false"));
    let out = run(
        home.path(),
        &["config", "memory", "profile", "../bad.sqlite"],
    );
    assert!(!out.status.success());
    let out = run(home.path(), &["memory", "list"]);
    assert!(out.status.success());
    assert!(home.path().join(".lai/memories/work.sqlite").exists());
    Ok(())
}
