use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

fn run_download(
    tags: &str,
    dotenv: Option<&str>,
    env_token: Option<&str>,
    expected_token: &str,
    unknown_size: bool,
) -> (std::process::Output, std::path::PathBuf) {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("test setup or operation should succeed");
    let endpoint = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("test setup or operation should succeed")
    );
    let tags = tags.to_owned();
    let expected_token = expected_token.to_owned();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener
                .accept()
                .expect("test setup or operation should succeed");
            let mut request = [0; 4096];
            let count = stream
                .read(&mut request)
                .expect("test setup or operation should succeed");
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(
                request
                    .to_lowercase()
                    .contains(&format!("authorization: bearer {}", expected_token).to_lowercase())
            );
            let body = if request.starts_with("GET /api/models/owner/model ") {
                format!(
                    r#"{{"sha":"0123456789012345678901234567890123456789","tags":{tags},"siblings":[{{"rfilename":"weights/model.safetensors"}}]}}"#
                )
            } else {
                assert!(request.starts_with("GET /owner/model/resolve/0123456789012345678901234567890123456789/weights/model.safetensors "), "{request}");
                "model data".to_owned()
            };
            if unknown_size && !request.starts_with("GET /api/models/") {
                write!(stream, "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}")
                    .expect("mock server should send response");
            } else {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .expect("mock server should send response");
            }
        }
    });
    let home = std::env::temp_dir().join(format!(
        "lai-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test setup or operation should succeed")
            .as_nanos()
    ));
    fs::create_dir_all(&home).expect("test setup or operation should succeed");
    if let Some(contents) = dotenv {
        fs::write(home.join(".env"), contents).expect("test setup or operation should succeed");
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_lai"));
    command.current_dir(&home);
    if let Some(token) = env_token {
        command.env("HF_TOKEN", token);
    } else {
        command.env_remove("HF_TOKEN");
    }
    let output = command
        .args(["download", "owner/model"])
        .env("HOME", &home)
        .env("HF_ENDPOINT", endpoint)
        .env_remove("RUST_LOG")
        .output()
        .expect("test setup or operation should succeed");
    server
        .join()
        .expect("test setup or operation should succeed");
    (output, home)
}

#[test]
fn downloads_a_tagged_model_and_reports_the_location() {
    let (output, home) = run_download(r#"["mlx"]"#, None, Some("test-token"), "test-token", false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let model = home.join(".lai/models/owner/model");
    assert_eq!(
        fs::read(model.join("weights/model.safetensors"))
            .expect("test setup or operation should succeed"),
        b"model data"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(&model.display().to_string()));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("WARN"));
    fs::remove_dir_all(home).expect("test setup or operation should succeed");
}

#[test]
fn warns_when_mlx_compatibility_is_not_marked() {
    let (output, home) = run_download("[]", None, Some("test-token"), "test-token", false);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("WARN"));
    assert!(
        home.join(".lai/models/owner/model/weights/model.safetensors")
            .exists()
    );
    fs::remove_dir_all(home).expect("test setup or operation should succeed");
}

#[test]
fn rejects_invalid_model_ids() {
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["download", "../model"])
        .output()
        .expect("test setup or operation should succeed");
    assert!(!output.status.success());
}

#[test]
fn reads_token_from_current_directory_dotenv() {
    let (output, home) = run_download(
        "[]",
        Some("HF_TOKEN=dotenv-token\n"),
        None,
        "dotenv-token",
        false,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(home).expect("test setup or operation should succeed");
}

#[test]
fn environment_token_takes_precedence_over_dotenv() {
    let (output, home) = run_download(
        "[]",
        Some("HF_TOKEN=dotenv-token\n"),
        Some("env-token"),
        "env-token",
        false,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(home).expect("test setup or operation should succeed");
}

#[test]
fn downloads_when_server_does_not_report_file_size() {
    let (output, home) = run_download("[]", None, Some("test-token"), "test-token", true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(home.join(".lai/models/owner/model/weights/model.safetensors"))
            .expect("downloaded file should exist"),
        b"model data"
    );
    fs::remove_dir_all(home).expect("test directory should be removed");
}

#[test]
fn invalid_endpoint_explains_how_to_fix_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["download", "owner/model"])
        .env("HF_ENDPOINT", "not-a-url")
        .output()
        .expect("CLI should start");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("HF_ENDPOINT") && error.contains("unset it"),
        "{error}"
    );
}

#[test]
fn missing_home_explains_how_to_fix_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(["download", "owner/model"])
        .env_remove("HOME")
        .output()
        .expect("CLI should start");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("HOME") && error.contains("set it"),
        "{error}"
    );
}
