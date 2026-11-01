use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "macos")]
static REQUEST_ID: AtomicU64 = AtomicU64::new(0);

use anyhow::{Context, ensure};

#[cfg(target_os = "macos")]
use safemlx::{Device, DeviceType, ExecutionContext};
#[cfg(target_os = "macos")]
use safemlx_lm::models::{
    LoadedModel,
    input::{InputPart, ModelInput},
};
#[cfg(target_os = "macos")]
use serde::Deserialize;
#[cfg(target_os = "macos")]
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use tiny_http::{Method, Response, Server, StatusCode};

pub fn run(detached: bool, port: u16, ready_signal: bool) -> anyhow::Result<()> {
    ensure!(
        port != 0,
        "port must be between 1 and 65535; choose a port with --port"
    );
    let (model_id, model_path) = crate::model::selected_model()?;
    if detached {
        return start_detached(port);
    }
    run_server(&model_id, &model_path, port, ready_signal)
}

#[cfg(target_os = "macos")]
fn start_detached(port: u16) -> anyhow::Result<()> {
    use std::{
        fs::{self, OpenOptions},
        os::unix::process::CommandExt,
        path::PathBuf,
    };
    let home =
        std::env::var_os("HOME").context("HOME is not set; set it to your home directory")?;
    let directory = PathBuf::from(home).join(".lai");
    fs::create_dir_all(&directory).context("cannot create ~/.lai; check directory permissions")?;
    let log_path = directory.join("serve.log");
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("cannot open {}; check permissions", log_path.display()))?;
    let executable = std::env::current_exe().context("cannot find the LAI executable")?;
    let mut command = Command::new(executable);
    command
        .args(["serve", "--port", &port.to_string(), "--ready-signal"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log_file));
    // A new session keeps the server running when the invoking terminal closes.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .context("cannot start the detached server")?;
    let stdout = child
        .stdout
        .take()
        .context("cannot read detached server startup status")?;
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .context("cannot read detached server startup status")?;
    ensure!(
        line.trim() == "ready",
        "server failed to start; inspect {} for details",
        log_path.display()
    );
    log::info!(
        "Server running at http://127.0.0.1:{port}/v1 (PID {}); logs: {}",
        child.id(),
        log_path.display()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn start_detached(_: u16) -> anyhow::Result<()> {
    anyhow::bail!("MLX inference requires Apple Silicon and macOS")
}

#[cfg(target_os = "macos")]
fn run_server(
    model_id: &str,
    model_path: &std::path::Path,
    port: u16,
    ready_signal: bool,
) -> anyhow::Result<()> {
    ensure!(
        cfg!(target_arch = "aarch64"),
        "MLX inference requires an Apple Silicon Mac"
    );
    let server = Server::http(("127.0.0.1", port)).map_err(|error| {
        anyhow::anyhow!(
            "cannot listen on 127.0.0.1:{port}: {error}; choose another port with --port"
        )
    })?;
    let context = ExecutionContext::new(Device::new(DeviceType::Gpu, 0));
    let weights_context = ExecutionContext::new(Device::new(DeviceType::Cpu, 0));
    let stream = context.stream();
    log::info!("Loading {model_id} from {}...", model_path.display());
    let mut model = LoadedModel::load(model_path, stream, weights_context.stream())
        .with_context(|| format!("cannot load model {model_id}; verify it is a supported MLX model and download it again if incomplete"))?;
    log::info!("Serving {model_id} at http://127.0.0.1:{port}/v1");
    if ready_signal {
        println!("ready");
        std::io::stdout()
            .flush()
            .context("cannot report server startup")?;
    }
    for request in server.incoming_requests() {
        handle_request(request, model_id, &mut model, stream);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn run_server(_: &str, _: &std::path::Path, _: u16, _: bool) -> anyhow::Result<()> {
    anyhow::bail!("MLX inference requires Apple Silicon and macOS")
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: Option<f32>,
    max_tokens: Option<usize>,
    max_completion_tokens: Option<usize>,
    #[serde(default)]
    stream: bool,
    n: Option<u32>,
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Message {
    role: String,
    content: String,
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompletionRequest {
    model: String,
    prompt: String,
    temperature: Option<f32>,
    max_tokens: Option<usize>,
    #[serde(default)]
    stream: bool,
    n: Option<u32>,
}

#[cfg(target_os = "macos")]
fn handle_request(
    mut request: tiny_http::Request,
    model_id: &str,
    model: &mut LoadedModel,
    stream: &safemlx::Stream,
) {
    if request.method() == &Method::Get && request.url() == "/v1/models" {
        respond(
            request,
            200,
            json!({"object":"list","data":[{"id":model_id,"object":"model","created":0,"owned_by":"local"}]}),
        );
        return;
    }
    let completion = request.url() == "/v1/completions";
    if request.method() != &Method::Post || (!completion && request.url() != "/v1/chat/completions")
    {
        respond_error(request, 404, "endpoint not found");
        return;
    }
    let mut body = Vec::new();
    if request
        .as_reader()
        .take(1_048_577)
        .read_to_end(&mut body)
        .is_err()
        || body.len() > 1_048_576
    {
        respond_error(
            request,
            400,
            "request body could not be read or exceeds 1 MiB",
        );
        return;
    }
    if completion {
        handle_completion(request, &body, model_id, model, stream);
        return;
    }
    let input: ChatRequest = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(error) => {
            respond_error(request, 400, &format!("invalid request: {error}"));
            return;
        }
    };
    if input.model != model_id {
        respond_error(request, 404, "requested model is not currently loaded");
        return;
    }
    if input.messages.is_empty()
        || input
            .messages
            .iter()
            .any(|m| !matches!(m.role.as_str(), "system" | "user" | "assistant"))
    {
        respond_error(
            request,
            400,
            "messages must contain text with system, user, or assistant roles",
        );
        return;
    }
    let temperature = match input.temperature {
        Some(value) => value,
        None => 1.0,
    };
    let max_tokens = match input.max_completion_tokens.or(input.max_tokens) {
        Some(value) => value,
        None => 256,
    };
    if !(0.0..=2.0).contains(&temperature)
        || !(1..=4096).contains(&max_tokens)
        || input.n.is_some_and(|n| n != 1)
    {
        respond_error(
            request,
            400,
            "temperature must be 0–2, max_tokens 1–4096, and n must be 1",
        );
        return;
    }
    match generate(model, stream, &input.messages, temperature, max_tokens) {
        Ok((text, prompt_tokens, completion_tokens, finish)) => {
            let created = unix_time();
            let id = format!(
                "chatcmpl-{}-{}-{}",
                created,
                std::process::id(),
                REQUEST_ID.fetch_add(1, Ordering::Relaxed)
            );
            if input.stream {
                let chunk = json!({"id":id,"object":"chat.completion.chunk","created":created,"model":model_id,"choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]});
                let end = json!({"id":id,"object":"chat.completion.chunk","created":created,"model":model_id,"choices":[{"index":0,"delta":{},"finish_reason":finish}]});
                let payload = format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n");
                send(request, 200, payload, "text/event-stream");
            } else {
                respond(
                    request,
                    200,
                    json!({"id":id,"object":"chat.completion","created":created,"model":model_id,"choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":finish}],"usage":{"prompt_tokens":prompt_tokens,"completion_tokens":completion_tokens,"total_tokens":prompt_tokens+completion_tokens}}),
                );
            }
        }
        Err(error) => {
            log::error!("Inference failed: {error:#}");
            respond_error(
                request,
                500,
                "model inference failed; check the server logs",
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn handle_completion(
    request: tiny_http::Request,
    body: &[u8],
    model_id: &str,
    model: &mut LoadedModel,
    stream: &safemlx::Stream,
) {
    let input: CompletionRequest = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(error) => {
            respond_error(request, 400, &format!("invalid request: {error}"));
            return;
        }
    };
    if input.model != model_id {
        respond_error(request, 404, "requested model is not currently loaded");
        return;
    }
    let temperature = match input.temperature {
        Some(value) => value,
        None => 1.0,
    };
    let max_tokens = match input.max_tokens {
        Some(value) => value,
        None => 256,
    };
    if input.prompt.is_empty()
        || !(0.0..=2.0).contains(&temperature)
        || !(1..=4096).contains(&max_tokens)
        || input.n.is_some_and(|n| n != 1)
    {
        respond_error(
            request,
            400,
            "prompt must not be empty, temperature must be 0–2, max_tokens 1–4096, and n must be 1",
        );
        return;
    }
    match generate_text(model, stream, &input.prompt, temperature, max_tokens) {
        Ok((text, prompt_tokens, completion_tokens, finish)) => {
            let created = unix_time();
            let id = format!(
                "cmpl-{}-{}-{}",
                created,
                std::process::id(),
                REQUEST_ID.fetch_add(1, Ordering::Relaxed)
            );
            if input.stream {
                let chunk = json!({"id":id,"object":"text_completion","created":created,"model":model_id,"choices":[{"index":0,"text":text,"logprobs":null,"finish_reason":finish}]});
                send(
                    request,
                    200,
                    format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                    "text/event-stream",
                );
            } else {
                respond(
                    request,
                    200,
                    json!({"id":id,"object":"text_completion","created":created,"model":model_id,"choices":[{"index":0,"text":text,"logprobs":null,"finish_reason":finish}],"usage":{"prompt_tokens":prompt_tokens,"completion_tokens":completion_tokens,"total_tokens":prompt_tokens+completion_tokens}}),
                );
            }
        }
        Err(error) => {
            log::error!("Inference failed: {error:#}");
            respond_error(
                request,
                500,
                "model inference failed; check the server logs",
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn generate(
    model: &mut LoadedModel,
    stream: &safemlx::Stream,
    messages: &[Message],
    temperature: f32,
    max_tokens: usize,
) -> anyhow::Result<(String, usize, usize, &'static str)> {
    let gemma = matches!(model.model_id_for_template(), "gemma4" | "gemma4_text");
    let messages: Vec<Value> = messages.iter().map(|message| {
        if gemma { json!({"role":message.role,"content":[{"type":"text","text":message.content,"content":message.content}]}) }
        else { json!({"role":message.role,"content":message.content}) }
    }).collect();
    let prompt = model
        .apply_chat_template_json(vec![messages], None, true)?
        .context("model has no chat template; use a model with a tokenizer chat template")?;
    generate_text(model, stream, &prompt, temperature, max_tokens)
}

#[cfg(target_os = "macos")]
fn generate_text(
    model: &mut LoadedModel,
    stream: &safemlx::Stream,
    prompt: &str,
    temperature: f32,
    max_tokens: usize,
) -> anyhow::Result<(String, usize, usize, &'static str)> {
    let prompt_ids = model.encode(prompt, false)?;
    let tokens = model.encode_to_array(prompt, false, stream)?;
    let eos = model.eos_token_ids().to_vec();
    let mut cache = model.new_cache();
    let key = if temperature == 0.0 {
        None
    } else {
        Some(safemlx::random::key(unix_time())?)
    };
    let mut generated = Vec::new();
    let mut finish = "length";
    {
        let parts = [InputPart::text_token_ids(&tokens)];
        let input = ModelInput::new(&parts);
        let mut generator =
            model.generate_input_with_cache(&mut cache, temperature, input, key, stream);
        for _ in 0..max_tokens {
            let token = match generator.next() {
                Some(token) => token?,
                None => {
                    finish = "stop";
                    break;
                }
            };
            let token = token.item::<u32>(stream);
            if eos.contains(&token) {
                finish = "stop";
                break;
            }
            generated.push(token);
        }
    }
    Ok((
        model.decode(&generated, true)?,
        prompt_ids.len(),
        generated.len(),
        finish,
    ))
}

#[cfg(target_os = "macos")]
fn unix_time() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}
#[cfg(target_os = "macos")]
fn respond(request: tiny_http::Request, status: u16, value: Value) {
    send(request, status, value.to_string(), "application/json");
}
#[cfg(target_os = "macos")]
fn respond_error(request: tiny_http::Request, status: u16, message: &str) {
    respond(
        request,
        status,
        json!({"error":{"message":message,"type":"invalid_request_error","param":null,"code":null}}),
    );
}
#[cfg(target_os = "macos")]
fn send(request: tiny_http::Request, status: u16, body: String, content_type: &str) {
    let mut response = Response::from_string(body).with_status_code(StatusCode(status));
    if let Ok(header) = tiny_http::Header::from_bytes("Content-Type", content_type) {
        response.add_header(header);
    }
    if let Err(error) = request.respond(response) {
        log::warn!("Could not send response: {error}");
    }
}
