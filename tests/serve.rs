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

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an Apple Silicon Mac and LAI_TEST_MODEL pointing to a downloaded model"]
fn serves_inference_endpoints() -> anyhow::Result<()> {
    use std::{
        net::TcpListener,
        os::unix::fs::symlink,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };

    struct Server(std::process::Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let model_path = std::env::var_os("LAI_TEST_MODEL")
        .ok_or_else(|| anyhow::anyhow!("set LAI_TEST_MODEL to a downloaded model"))?;
    let home = tempfile::tempdir()?;
    let directory = home.path().join(".lai/models/owner");
    fs::create_dir_all(&directory)?;
    symlink(model_path, directory.join("model"))?;
    fs::write(
        home.path().join(".lai/config.toml"),
        "model = \"owner/model\"\nsystem_prompt = \"Follow these default instructions.\"\n",
    )?;
    let socket = TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    drop(socket);
    let logs = home.path().join("serve.log");
    let child = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["serve", "--port", &port.to_string()])
        .env("HOME", home.path())
        .env("RUST_LOG", "lai=info")
        .stdout(Stdio::null())
        .stderr(Stdio::from(fs::File::create(&logs)?))
        .spawn()?;
    let mut server = Server(child);
    let client = reqwest::blocking::Client::new();
    let base = format!("http://127.0.0.1:{port}/v1");
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if client.get(format!("{base}/models")).send().is_ok() {
            break;
        }
        if let Some(status) = server.0.try_wait()? {
            anyhow::bail!("server exited {status}: {}", fs::read_to_string(&logs)?);
        }
        anyhow::ensure!(Instant::now() < deadline, "server did not become ready");
        thread::sleep(Duration::from_millis(200));
    }
    for (endpoint, body) in [
        (
            "chat/completions",
            serde_json::json!({"model":"owner/model","messages":[{"role":"user","content":"private prompt"}],"max_tokens":2,"temperature":0}),
        ),
        (
            "completions",
            serde_json::json!({"model":"owner/model","prompt":"private prompt","max_tokens":2,"temperature":0}),
        ),
        (
            "responses",
            serde_json::json!({"model":"owner/model","input":"private prompt","max_output_tokens":2,"temperature":0}),
        ),
    ] {
        let response = client
            .post(format!("{base}/{endpoint}"))
            .json(&body)
            .send()?;
        anyhow::ensure!(
            response.status().is_success(),
            "{endpoint} returned {}",
            response.status()
        );
    }
    let result: serde_json::Value = client.post(format!("{base}/responses"))
        .json(&serde_json::json!({"model":"owner/model","input":"Hi","max_output_tokens":1,"temperature":0}))
        .send()?.error_for_status()?.json()?;
    assert_eq!(result["object"], "response");
    assert_eq!(result["output"][0]["role"], "assistant");
    assert!(result["usage"]["input_tokens"].as_u64().unwrap() > 0);
    let streamed = client.post(format!("{base}/responses"))
        .json(&serde_json::json!({"model":"owner/model","input":"Hi","max_output_tokens":1,"temperature":0,"stream":true}))
        .send()?.error_for_status()?.text()?;
    assert!(streamed.contains("event: response.created\n"));
    assert!(streamed.contains("event: response.output_text.delta\n"));
    assert!(
        streamed.contains("event: response.incomplete\n")
            || streamed.contains("event: response.completed\n")
    );
    let rejected = client.post(format!("{base}/responses"))
        .json(&serde_json::json!({"model":"owner/model","input":"Hi","tools":[{"type":"web_search"}]}))
        .send()?;
    assert_eq!(rejected.status(), 400);
    let mut prompt_counts = Vec::new();
    for messages in [
        serde_json::json!([{"role":"user","content":"Hi"}]),
        serde_json::json!([{"role":"system","content":"Follow these default instructions."},{"role":"user","content":"Hi"}]),
        serde_json::json!([{"role":"system","content":"Different instructions that are deliberately longer than the default system prompt."},{"role":"user","content":"Hi"}]),
    ] {
        let response: serde_json::Value = client.post(format!("{base}/chat/completions"))
            .json(&serde_json::json!({"model":"owner/model","messages":messages,"max_tokens":1,"temperature":0}))
            .send()?.error_for_status()?.json()?;
        prompt_counts.push(response["usage"]["prompt_tokens"].as_u64().unwrap());
    }
    assert_eq!(
        prompt_counts[0], prompt_counts[1],
        "default prompt should be inserted exactly once"
    );
    assert_ne!(
        prompt_counts[1], prompt_counts[2],
        "client system prompt should take precedence"
    );
    let logs = fs::read_to_string(logs)?;
    for endpoint in ["/v1/chat/completions", "/v1/completions", "/v1/responses"] {
        assert!(
            logs.contains(&format!("Starting inference for {endpoint}")),
            "{logs}"
        );
        assert!(
            logs.contains(&format!("Finished inference for {endpoint}")),
            "{logs}"
        );
        let finished = logs
            .lines()
            .find(|line| line.contains(&format!("Finished inference for {endpoint}")))
            .unwrap();
        assert!(
            finished.contains("tok/s including prompt processing"),
            "{finished}"
        );
    }
    assert!(!logs.contains("private prompt"));
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn kill_reports_when_no_background_server_exists() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .arg("kill")
        .env("HOME", home.path())
        .env("RUST_LOG", "lai=info")
        .output()
        .expect("CLI should start");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no detached LAI server"));
}

#[cfg(target_os = "macos")]
#[test]
fn kill_requests_shutdown_from_the_background_server() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        thread,
    };
    let home = tempfile::tempdir().expect("temporary home should be created");
    let directory = home.path().join(".lai");
    fs::create_dir(&directory).expect("LAI directory should be created");
    let listener =
        UnixListener::bind(directory.join("serve.sock")).expect("control socket should bind");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("shutdown connection should arrive");
        let mut command = String::new();
        BufReader::new(stream.try_clone().expect("socket should clone"))
            .read_line(&mut command)
            .expect("command should be read");
        assert_eq!(command, "shutdown\n");
        stream
            .write_all(b"stopping\n")
            .expect("response should be sent");
    });
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .arg("kill")
        .env("HOME", home.path())
        .env("RUST_LOG", "lai=info")
        .output()
        .expect("CLI should start");
    server.join().expect("mock server should finish");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Stopping"));
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an Apple Silicon Mac and LAI_TEST_MODEL pointing to a downloaded model"]
fn detached_server_can_be_stopped_and_restarted() -> anyhow::Result<()> {
    use std::{
        net::TcpListener,
        os::unix::{fs::symlink, net::UnixListener},
        thread,
        time::{Duration, Instant},
    };
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = Command::new(env!("CARGO_BIN_EXE_lai"))
                .arg("kill")
                .env("HOME", &self.0)
                .output();
        }
    }
    let model = std::env::var_os("LAI_TEST_MODEL")
        .ok_or_else(|| anyhow::anyhow!("set LAI_TEST_MODEL to a downloaded model"))?;
    let home = tempfile::tempdir()?;
    let _cleanup = Cleanup(home.path().to_path_buf());
    let directory = home.path().join(".lai/models/owner");
    fs::create_dir_all(&directory)?;
    symlink(model, directory.join("model"))?;
    fs::write(
        home.path().join(".lai/config.toml"),
        "model = \"owner/model\"\n",
    )?;
    let socket = TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    drop(socket);
    let base = format!("http://127.0.0.1:{port}/v1/models");
    let client = reqwest::blocking::Client::new();
    let stale_socket = UnixListener::bind(home.path().join(".lai/serve.sock"))?;
    drop(stale_socket);
    for _ in 0..2 {
        let start = Command::new(env!("CARGO_BIN_EXE_lai"))
            .args(["serve", "-d", "--port", &port.to_string()])
            .env("HOME", home.path())
            .output()?;
        anyhow::ensure!(
            start.status.success(),
            "start failed: {}",
            String::from_utf8_lossy(&start.stderr)
        );
        anyhow::ensure!(
            client.get(&base).send()?.status().is_success(),
            "server is not reachable"
        );
        let duplicate = Command::new(env!("CARGO_BIN_EXE_lai"))
            .args(["serve", "-d", "--port", &port.to_string()])
            .env("HOME", home.path())
            .output()?;
        anyhow::ensure!(
            !duplicate.status.success(),
            "a second detached server should be rejected"
        );
        let stop = Command::new(env!("CARGO_BIN_EXE_lai"))
            .arg("kill")
            .env("HOME", home.path())
            .output()?;
        anyhow::ensure!(
            stop.status.success(),
            "shutdown failed: {}",
            String::from_utf8_lossy(&stop.stderr)
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        while home.path().join(".lai/serve.sock").exists() {
            anyhow::ensure!(Instant::now() < deadline, "server did not shut down");
            thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(())
}
