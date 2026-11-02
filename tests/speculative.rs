#![cfg(target_os = "macos")]
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::symlink,
    process::{Command, Stdio},
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

#[test]
#[ignore = "requires LAI_TEST_MODEL and LAI_TEST_DRAFT with compatible downloaded MLX models"]
fn speculative_output_matches_regular_greedy_output() -> anyhow::Result<()> {
    let target =
        std::env::var_os("LAI_TEST_MODEL").ok_or_else(|| anyhow::anyhow!("set LAI_TEST_MODEL"))?;
    let draft =
        std::env::var_os("LAI_TEST_DRAFT").ok_or_else(|| anyhow::anyhow!("set LAI_TEST_DRAFT"))?;
    let home = tempfile::tempdir()?;
    let dir = home.path().join(".lai");
    fs::create_dir_all(dir.join("models/owner"))?;
    symlink(target, dir.join("models/owner/target"))?;
    symlink(draft, dir.join("models/owner/draft"))?;
    let client = reqwest::blocking::Client::new();
    let mut outputs = Vec::new();
    for enabled in [false, true] {
        fs::write(
            dir.join("config.toml"),
            format!(
                "model = \"owner/target\"\ndraft_model = \"owner/draft\"\nspeculative_decoding = {enabled}\n"
            ),
        )?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let log_path = home.path().join(format!("server-{enabled}.log"));
        let mut server = Server(
            Command::new(env!("CARGO_BIN_EXE_lai"))
                .args(["serve", "--port", &port.to_string()])
                .env("HOME", home.path())
                .env("RUST_LOG", "lai=info")
                .stdout(Stdio::null())
                .stderr(Stdio::from(fs::File::create(&log_path)?))
                .spawn()?,
        );
        let base = format!("http://127.0.0.1:{port}/v1");
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if client.get(format!("{base}/models")).send().is_ok() {
                break;
            }
            if let Some(status) = server.0.try_wait()? {
                anyhow::bail!("server exited {status}: {}", fs::read_to_string(&log_path)?);
            }
            anyhow::ensure!(Instant::now() < deadline, "server did not become ready");
            thread::sleep(Duration::from_millis(200));
        }
        let result: serde_json::Value = client.post(format!("{base}/chat/completions"))
            .json(&serde_json::json!({"model":"owner/target","messages":[{"role":"user","content":"What is the capital of France?"}],"temperature":0,"max_tokens":32}))
            .send()?.error_for_status()?.json()?;
        outputs.push(result["choices"][0]["message"]["content"].clone());
        if enabled {
            assert!(fs::read_to_string(&log_path)?.contains("Speculative decoding: accepted"));
            let response = client.post(format!("{base}/chat/completions"))
                .json(&serde_json::json!({"model":"owner/target","messages":[{"role":"user","content":"Hello"}],"temperature":0.5}))
                .send()?;
            assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        }
    }
    assert_eq!(outputs[0], outputs[1]);
    Ok(())
}
