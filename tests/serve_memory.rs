#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires local chat and embedding models, an MLX Python runtime and Apple GPU"]
fn server_reindexes_and_queries_existing_memories() -> anyhow::Result<()> {
    use lai::memory::{MemoryStore, NewMemory};
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

    let chat =
        std::env::var_os("LAI_TEST_MODEL").ok_or_else(|| anyhow::anyhow!("set LAI_TEST_MODEL"))?;
    let embedding = std::env::var_os("LAI_TEST_EMBEDDING_MODEL")
        .ok_or_else(|| anyhow::anyhow!("set LAI_TEST_EMBEDDING_MODEL"))?;
    let python = std::env::var_os("LAI_MEMORY_PYTHON")
        .ok_or_else(|| anyhow::anyhow!("set LAI_MEMORY_PYTHON"))?;
    let home = tempfile::tempdir()?;
    let root = home.path().join(".lai");
    fs::create_dir_all(root.join("models/owner"))?;
    symlink(chat, root.join("models/owner/chat"))?;
    symlink(embedding, root.join("models/owner/embedding"))?;
    fs::write(
        root.join("config.toml"),
        "model = \"owner/chat\"\nmodel_memory = \"owner/embedding\"\n",
    )?;
    let path = root.join("memories/default.sqlite");
    let mut store = MemoryStore::open(&path)?;
    let id = store.insert(&NewMemory::new("User likes cycling", "I like cycling"))?;
    drop(store);
    let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
    let log = root.join("test.log");
    let child = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["serve", "--port", &port.to_string()])
        .env("HOME", home.path())
        .env("LAI_MEMORY_PYTHON", python)
        .stdout(Stdio::null())
        .stderr(Stdio::from(fs::File::create(&log)?))
        .spawn()?;
    let mut server = Server(child);
    let client = reqwest::blocking::Client::new();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = server.0.try_wait()? {
            anyhow::bail!("server exited {status}: {}", fs::read_to_string(&log)?);
        }
        let conn = rusqlite::Connection::open(&path)?;
        let count: i64 =
            conn.query_row("SELECT count(*) FROM memory_embeddings", [], |r| r.get(0))?;
        if count == 1 {
            break;
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "memory was not embedded: {}",
            fs::read_to_string(&log)?
        );
        thread::sleep(Duration::from_millis(250));
    }
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let request = serde_json::json!({"model":"owner/chat","messages":[{"role":"user","content":"What outdoor activities do I enjoy?"}],"max_tokens":2,"temperature":0});
    let with_memory: serde_json::Value = client
        .post(&url)
        .json(&request)
        .send()?
        .error_for_status()?
        .json()?;
    MemoryStore::open(&path)?.delete(id)?;
    let without_memory: serde_json::Value = client
        .post(&url)
        .json(&request)
        .send()?
        .error_for_status()?
        .json()?;
    assert!(
        with_memory["usage"]["prompt_tokens"].as_u64()
            > without_memory["usage"]["prompt_tokens"].as_u64()
    );
    assert!(!fs::read_to_string(&log)?.contains("Memory vector search failed"));
    Ok(())
}
