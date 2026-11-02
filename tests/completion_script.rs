#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("script/completion.sh")
}

#[test]
fn requires_a_message() {
    let output = Command::new(script())
        .output()
        .expect("script should start");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn sends_the_message_as_json_using_the_loaded_model() {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let fake_curl = temp.path().join("curl");
    fs::write(
        &fake_curl,
        r##"#!/bin/sh
case "$*" in
  *'/models'*) printf '%s' '{"data":[{"id":"owner/model"}]}' ;;
  *)
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "--data" ]; then shift; printf '%s' "$1" > "$REQUEST_FILE"; fi
      shift
    done
    printf '%s' '{"choices":[{"message":{"content":"Hello back!"}}]}' ;;
esac
"##,
    )
    .expect("mock curl should be written");
    fs::set_permissions(&fake_curl, fs::Permissions::from_mode(0o755))
        .expect("mock curl should be executable");
    let request_path = temp.path().join("request.json");
    let existing_path = std::env::var_os("PATH").expect("PATH should be available");
    let path = std::env::join_paths(
        std::iter::once(temp.path().to_path_buf()).chain(std::env::split_paths(&existing_path)),
    )
    .expect("PATH should be valid");
    let message = "A \"quote\"\nand a newline";
    let output = Command::new(script())
        .arg(message)
        .env("PATH", path)
        .env("REQUEST_FILE", &request_path)
        .output()
        .expect("script should start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "Hello back!"
    );
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(request_path).expect("request should be recorded"))
            .expect("request should be valid JSON");
    assert_eq!(request["model"], "owner/model");
    assert_eq!(request["messages"][0]["content"], message);
}

#[test]
fn installs_missing_jq_with_homebrew() {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let bin = temp.path();
    fs::write(bin.join("brew"), r##"#!/bin/sh
[ "$1" = install ] && [ "$2" = jq ] || exit 1
printf '%s' installed > "$TEST_BIN/brew-called"
printf '%s\n' '#!/bin/sh' 'case "$*" in *data*) printf "owner/model\\n" ;; *choices*) printf "Hello back!\\n" ;; *) printf "{}\\n" ;; esac' > "$TEST_BIN/jq"
/bin/chmod +x "$TEST_BIN/jq"
"##).expect("mock brew should be written");
    fs::write(bin.join("curl"), "#!/bin/sh\nprintf '%s' '{}'\n")
        .expect("mock curl should be written");
    for name in ["brew", "curl"] {
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755))
            .expect("mock should be executable");
    }
    let output = Command::new(script())
        .arg("Hello")
        .env("PATH", bin)
        .env("TEST_BIN", bin)
        .output()
        .expect("script should start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(bin.join("brew-called").exists());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "Hello back!"
    );
}

#[test]
fn missing_homebrew_has_an_actionable_error() {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let output = Command::new(script())
        .arg("Hello")
        .env("PATH", temp.path())
        .output()
        .expect("script should start");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("Homebrew") && error.contains("https://brew.sh"),
        "{error}"
    );
}

#[test]
fn reports_when_homebrew_install_fails() {
    let temp = tempfile::tempdir().expect("temporary directory should be created");
    let brew = temp.path().join("brew");
    fs::write(&brew, "#!/bin/sh\nexit 1\n").expect("mock brew should be written");
    fs::set_permissions(&brew, fs::Permissions::from_mode(0o755))
        .expect("mock brew should be executable");
    let output = Command::new(script())
        .arg("Hello")
        .env("PATH", temp.path())
        .output()
        .expect("script should start");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("brew install jq"));
}
