use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
    thread,
};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lai"))
        .env("HOME", home)
        .env_remove("RUST_LOG")
        .args(args)
        .output()
        .expect("CLI should start")
}

#[test]
fn set_update_and_unset_preserve_other_configuration() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let path = home.path().join(".lai/config.toml");
    fs::create_dir_all(
        home.path()
            .join(".lai/models/DreamFoundries/supergemma4-e4b-abliterated-6bit"),
    )
    .expect("installed model directory should be created");
    let set = run(
        home.path(),
        &[
            "config",
            "model",
            "set",
            "DreamFoundries/supergemma4-e4b-abliterated-6bit",
        ],
    );
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    let original = fs::read_to_string(&path).expect("config should exist");
    assert!(original.contains("model = \"DreamFoundries/supergemma4-e4b-abliterated-6bit\""));
    fs::write(
        &path,
        format!("# Keep this comment\n{original}other = 42\n"),
    )
    .expect("fixture should be written");
    let duplicate = run(home.path(), &["config", "model", "set", "owner/other"]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("update"));
    let update = run(home.path(), &["config", "model", "update", "owner/other"]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    let updated = fs::read_to_string(&path).expect("config should be readable");
    assert!(updated.contains("model = \"owner/other\""));
    assert!(updated.contains("# Keep this comment") && updated.contains("other = 42"));
    let unset = run(home.path(), &["config", "model", "unset"]);
    assert!(unset.status.success());
    let remaining = fs::read_to_string(&path).expect("config should be readable");
    assert!(!remaining.contains("model ="));
    assert!(remaining.contains("# Keep this comment") && remaining.contains("other = 42"));
}

fn set_with_response(home: &Path, answer: &str, endpoint: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["config", "model", "set", "owner/model"])
        .current_dir(home)
        .env("HOME", home)
        .env("HF_ENDPOINT", endpoint)
        .env("HF_TOKEN", "test-token")
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI should start");
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(answer.as_bytes())
            .expect("answer should be sent");
    }
    child.wait_with_output().expect("CLI should finish")
}

#[test]
fn missing_model_warns_and_can_be_selected_without_download() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let output = set_with_response(home.path(), "n\n", "http://127.0.0.1:1");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("WARN")
            && stderr.contains("not installed")
            && stderr.contains("Download this model now?"),
        "{stderr}"
    );
    assert!(
        fs::read_to_string(home.path().join(".lai/config.toml"))
            .expect("config should exist")
            .contains("owner/model")
    );
}

#[test]
fn missing_model_can_be_downloaded_before_selection() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let server = TcpListener::bind("127.0.0.1:0").expect("mock server should bind");
    let endpoint = format!(
        "http://{}",
        server.local_addr().expect("server should have address")
    );
    let handle = thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = server.accept().expect("request should arrive");
            let mut request = [0; 4096];
            let bytes_read = stream
                .read(&mut request)
                .expect("request should be readable");
            assert!(bytes_read > 0, "request should not be empty");
            let body = if index == 0 {
                r#"{"sha":"0123456789012345678901234567890123456789","tags":["mlx"],"siblings":[{"rfilename":"model.safetensors"}]}"#
            } else {
                "model data"
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response should be sent");
        }
    });
    let output = set_with_response(home.path(), "y\n", &endpoint);
    handle.join().expect("mock server should finish");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(
            home.path()
                .join(".lai/models/owner/model/model.safetensors")
        )
        .expect("model should be downloaded"),
        b"model data"
    );
    assert!(home.path().join(".lai/config.toml").exists());
}

#[test]
fn missing_model_without_response_does_not_save_selection() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let output = set_with_response(home.path(), "", "http://127.0.0.1:1");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no response"));
    assert!(!home.path().join(".lai/config.toml").exists());
}

#[test]
fn failed_download_does_not_save_selection() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let output = set_with_response(home.path(), "y\n", "invalid-url");
    assert!(!output.status.success());
    assert!(!home.path().join(".lai/config.toml").exists());
}

#[test]
fn rejects_missing_selection_and_invalid_model_without_creating_config() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    for args in [
        vec!["config", "model", "update", "owner/model"],
        vec!["config", "model", "unset"],
        vec!["config", "model", "set", "../bad"],
    ] {
        let output = run(home.path(), &args);
        assert!(!output.status.success());
    }
    assert!(!home.path().join(".lai/config.toml").exists());
}

#[test]
fn malformed_config_is_not_overwritten() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let directory = home.path().join(".lai");
    fs::create_dir(&directory).expect("config directory should be created");
    let path = directory.join("config.toml");
    fs::write(&path, "model = [\n").expect("fixture should be written");
    let output = run(home.path(), &["config", "model", "set", "owner/model"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fix the file"));
    assert_eq!(
        fs::read_to_string(path).expect("config should be readable"),
        "model = [\n"
    );
}

#[test]
fn can_enable_and_disable_speculative_decoding() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let directory = home.path().join(".lai");
    let help = run(home.path(), &["config", "--help"]);
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("speculativeDecoding") && help.contains("systemPrompt"));
    assert!(
        !run(home.path(), &["model", "speculativeDecoding", "false"])
            .status
            .success()
    );
    fs::create_dir_all(directory.join("models/owner/draft")).expect("draft directory should exist");
    fs::write(directory.join("config.toml"), "model = \"owner/target\"\n")
        .expect("config should be written");
    let output = run(
        home.path(),
        &[
            "config",
            "speculativeDecoding",
            "true",
            "--draft-model",
            "owner/draft",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(directory.join("config.toml")).expect("config should exist");
    assert!(
        contents.contains("model = \"owner/target\"")
            && contents.contains("draft_model = \"owner/draft\"")
            && contents.contains("speculative_decoding = true")
    );
    let output = run(home.path(), &["config", "speculativeDecoding", "false"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let contents = fs::read_to_string(directory.join("config.toml")).expect("config should exist");
    assert!(
        contents.contains("model = \"owner/target\"")
            && contents.contains("speculative_decoding = false")
    );
}

#[test]
fn system_prompt_can_be_set_replaced_and_cleared_without_losing_settings() {
    let home = tempfile::tempdir().unwrap();
    let directory = home.path().join(".lai");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("config.toml");
    fs::write(
        &path,
        "# keep\nmodel = 'owner/model'\n[pet.animations]\nidle = 'Blink'\n",
    )
    .unwrap();
    for prompt in ["Be kind.\nUse \"short\" answers.", "A new prompt"] {
        let output = run(home.path(), &["config", "systemPrompt", prompt]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let config: toml_edit::DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(config["system_prompt"].as_str(), Some(prompt));
        assert_eq!(config["model"].as_str(), Some("owner/model"));
        assert_eq!(config["pet"]["animations"]["idle"].as_str(), Some("Blink"));
        assert!(fs::read_to_string(&path).unwrap().contains("# keep"));
    }
    let output = run(home.path(), &["config", "systemPrompt", ""]);
    assert!(output.status.success());
    let config: toml_edit::DocumentMut = fs::read_to_string(path).unwrap().parse().unwrap();
    assert!(config.get("system_prompt").is_none());
}

#[test]
fn system_prompt_creates_config_and_does_not_replace_invalid_toml() {
    let home = tempfile::tempdir().unwrap();
    assert!(
        run(home.path(), &["config", "systemPrompt", "Hello"])
            .status
            .success()
    );
    let path = home.path().join(".lai/config.toml");
    fs::write(&path, "model = [\n").unwrap();
    let output = run(home.path(), &["config", "systemPrompt", "Changed"]);
    assert!(!output.status.success());
    assert_eq!(fs::read_to_string(path).unwrap(), "model = [\n");
}

#[test]
fn missing_speculative_draft_model_has_actionable_error_on_serve() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let directory = home.path().join(".lai");
    fs::create_dir_all(directory.join("models/owner/target"))
        .expect("target directory should exist");
    fs::write(
        directory.join("config.toml"),
        "model = \"owner/target\"\ndraft_model = \"owner/draft\"\nspeculative_decoding = true\n",
    )
    .expect("config should be written");
    let output = run(home.path(), &["serve"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("lai download owner/draft"));
}
