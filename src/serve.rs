use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicU64;
#[cfg(target_os = "macos")]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "macos")]
static REQUEST_ID: AtomicU64 = AtomicU64::new(0);

use anyhow::{Context, ensure};

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct InferenceProgress {
    last_report: usize,
}

#[cfg(any(target_os = "macos", test))]
impl InferenceProgress {
    fn rate(tokens: usize, elapsed: std::time::Duration) -> f64 {
        if elapsed.is_zero() {
            0.0
        } else {
            tokens as f64 / elapsed.as_secs_f64()
        }
    }

    fn update(&mut self, tokens: usize, elapsed: std::time::Duration) -> Option<f64> {
        if tokens.saturating_sub(self.last_report) < 10 {
            return None;
        }
        self.last_report = tokens;
        Some(Self::rate(tokens, elapsed))
    }
}

#[cfg(test)]
mod progress_tests {
    use super::InferenceProgress;
    use std::time::Duration;

    #[test]
    fn reports_progress_after_every_ten_additional_tokens() {
        let mut progress = InferenceProgress::default();
        assert_eq!(progress.update(2, Duration::from_secs(1)), None);
        assert_eq!(progress.update(9, Duration::from_secs(3)), None);
        assert_eq!(progress.update(10, Duration::from_secs(5)), Some(2.0));
        assert_eq!(progress.update(19, Duration::from_secs(9)), None);
        assert_eq!(progress.update(20, Duration::from_secs(10)), Some(2.0));
    }

    #[test]
    fn speculative_rounds_can_cross_a_reporting_threshold() {
        let mut progress = InferenceProgress::default();
        assert_eq!(progress.update(9, Duration::from_secs(2)), None);
        assert_eq!(progress.update(12, Duration::from_secs(3)), Some(4.0));
        assert_eq!(progress.update(21, Duration::from_secs(5)), None);
        assert_eq!(
            progress.update(23, Duration::from_secs(6)),
            Some(23.0 / 6.0)
        );
    }

    #[test]
    fn throughput_handles_zero_duration_and_no_tokens() {
        assert_eq!(InferenceProgress::rate(5, Duration::ZERO), 0.0);
        assert_eq!(InferenceProgress::rate(0, Duration::from_secs(2)), 0.0);
    }
}

#[cfg(test)]
mod system_prompt_tests {
    use super::{Message, apply_system_prompt};

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.into(),
            content: content.into(),
        }
    }

    #[test]
    fn default_prompt_is_prepended_only_when_client_has_no_system_message() {
        let mut messages = vec![msg("user", "Hi")];
        apply_system_prompt(&mut messages, Some("Be kind"));
        assert_eq!(
            (messages[0].role.as_str(), messages[0].content.as_str()),
            ("system", "Be kind")
        );
        assert_eq!(messages[1].content, "Hi");

        let mut messages = vec![msg("system", "Client instructions"), msg("user", "Hi")];
        apply_system_prompt(&mut messages, Some("Be kind"));
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Client instructions");

        let mut messages = vec![msg("user", "Hi")];
        apply_system_prompt(&mut messages, None);
        assert_eq!(messages.len(), 1);
    }
}

#[cfg(target_os = "macos")]
use lai::pet::lifecycle::{detached_arguments, run_pet_lifecycle, wait_for_pet_ui};

#[cfg(target_os = "macos")]
use safemlx::{Device, DeviceType, ExecutionContext};
#[cfg(target_os = "macos")]
use safemlx_lm::models::{
    LoadedModel, Model,
    gemma4_assistant::{Gemma4AssistantDraftModel, load_gemma4_assistant_model},
    input::{InputPart, ModelInput},
};
#[cfg(target_os = "macos")]
use serde::Deserialize;
#[cfg(target_os = "macos")]
use serde_json::{Value, json};
#[cfg(target_os = "macos")]
use tiny_http::{Method, Response, Server, StatusCode};

pub fn run(detached: bool, pet: bool, port: u16, ready_signal: bool) -> anyhow::Result<()> {
    ensure!(
        port != 0,
        "port must be between 1 and 65535; choose a port with --port"
    );
    let (model_id, model_path) = crate::model::selected_model()?;
    let draft = crate::model::selected_draft()?;
    if detached {
        return start_detached(port, pet);
    }
    if pet {
        return run_with_pet(model_id, model_path, draft, port, ready_signal);
    }
    run_server(
        &model_id,
        &model_path,
        draft.as_ref(),
        port,
        ready_signal,
        Default::default(),
        None,
    )
}

#[cfg(target_os = "macos")]
fn run_with_pet(
    model_id: String,
    model_path: std::path::PathBuf,
    draft: Option<(String, std::path::PathBuf)>,
    port: u16,
    ready_signal: bool,
) -> anyhow::Result<()> {
    let pet_ready = Arc::new(AtomicBool::new(false));
    let worker_ready = Arc::clone(&pet_ready);
    run_pet_lifecycle(
        move |shutdown| {
            run_server(
                &model_id,
                &model_path,
                draft.as_ref(),
                port,
                ready_signal,
                shutdown,
                Some(worker_ready),
            )
        },
        move |shutdown, done| crate::pet::run(port, shutdown, done, pet_ready),
    )
}

#[cfg(not(target_os = "macos"))]
fn run_with_pet(
    _: String,
    _: std::path::PathBuf,
    _: Option<(String, std::path::PathBuf)>,
    _: u16,
    _: bool,
) -> anyhow::Result<()> {
    anyhow::bail!("The desktop pet requires macOS")
}

#[cfg(target_os = "macos")]
pub fn kill() -> anyhow::Result<()> {
    crate::server_control::stop()
}

#[cfg(not(target_os = "macos"))]
pub fn kill() -> anyhow::Result<()> {
    anyhow::bail!("Detached server management requires macOS")
}

#[cfg(target_os = "macos")]
fn start_detached(port: u16, pet: bool) -> anyhow::Result<()> {
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
        .args(detached_arguments(port, pet))
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
fn start_detached(_: u16, _: bool) -> anyhow::Result<()> {
    anyhow::bail!("MLX inference requires Apple Silicon and macOS")
}

#[cfg(target_os = "macos")]
fn run_server(
    model_id: &str,
    model_path: &std::path::Path,
    draft: Option<&(String, std::path::PathBuf)>,
    port: u16,
    ready_signal: bool,
    stop_requested: Arc<AtomicBool>,
    pet_ready: Option<Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    ensure!(
        cfg!(target_arch = "aarch64"),
        "MLX inference requires an Apple Silicon Mac"
    );
    let system_prompt = crate::model::system_prompt()?;
    let mut control = if ready_signal {
        Some(crate::server_control::Control::acquire()?)
    } else {
        None
    };
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
    let mut assistant = if let Some((draft_id, draft_path)) = draft {
        let target = match model.model_mut() {
            Model::Gemma4(target) => target,
            _ => anyhow::bail!(
                "speculative decoding currently supports Gemma 4 models only; disable it with `lai config speculativeDecoding false`"
            ),
        };
        log::info!(
            "Loading speculative draft model {draft_id} from {}...",
            draft_path.display()
        );
        let assistant = load_gemma4_assistant_model(draft_path, stream, weights_context.stream())
            .with_context(|| {
            format!(
                "cannot load draft model {draft_id}; use a compatible unquantized Gemma 4 assistant"
            )
        })?;
        let draft_args = &assistant.config.text_config;
        let target_args = &target.args;
        ensure!(
            assistant.config.model_type == "gemma4_assistant"
                && assistant.config.backbone_hidden_size == target_args.hidden_size
                && draft_args.vocab_size == target_args.vocab_size
                && draft_args.num_key_value_heads == target_args.num_key_value_heads
                && draft_args.head_dim == target_args.head_dim
                && draft_args.global_head_dim == target_args.global_head_dim,
            "draft model {draft_id} is incompatible with {model_id}; choose a compatible Gemma 4 assistant"
        );
        log::info!("Speculative decoding enabled using {draft_id}");
        Some(assistant)
    } else {
        None
    };
    log::info!("Serving {model_id} at http://127.0.0.1:{port}/v1");
    if let Some(control) = &mut control {
        control.start(Arc::clone(&stop_requested))?;
    }
    if stop_requested.load(Ordering::Acquire)
        || (ready_signal
            && pet_ready
                .as_ref()
                .is_some_and(|ready| !wait_for_pet_ui(ready, &stop_requested)))
    {
        return Ok(());
    }
    if ready_signal {
        println!("ready");
        std::io::stdout()
            .flush()
            .context("cannot report server startup")?;
    }
    loop {
        if stop_requested.load(Ordering::Acquire) {
            log::info!("Stopping LAI server");
            break;
        }
        if let Some(request) = server.recv_timeout(std::time::Duration::from_millis(250))? {
            handle_request(
                request,
                model_id,
                &mut model,
                &mut assistant,
                stream,
                system_prompt.as_deref(),
            );
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn run_server(
    _: &str,
    _: &std::path::Path,
    _: Option<&(String, std::path::PathBuf)>,
    _: u16,
    _: bool,
    _: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> anyhow::Result<()> {
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

#[cfg(any(target_os = "macos", test))]
#[cfg_attr(target_os = "macos", derive(Deserialize))]
#[cfg_attr(target_os = "macos", serde(deny_unknown_fields))]
struct Message {
    role: String,
    content: String,
}

#[cfg(any(target_os = "macos", test))]
fn apply_system_prompt(messages: &mut Vec<Message>, prompt: Option<&str>) {
    if let Some(prompt) =
        prompt.filter(|_| !messages.iter().any(|message| message.role == "system"))
    {
        messages.insert(
            0,
            Message {
                role: "system".into(),
                content: prompt.into(),
            },
        );
    }
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
#[allow(clippy::manual_unwrap_or)] // Defaults are intentional for optional API fields.
fn handle_request(
    mut request: tiny_http::Request,
    model_id: &str,
    model: &mut LoadedModel,
    assistant: &mut Option<Gemma4AssistantDraftModel>,
    stream: &safemlx::Stream,
    system_prompt: Option<&str>,
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
        handle_completion(request, &body, model_id, model, assistant, stream);
        return;
    }
    let mut input: ChatRequest = match serde_json::from_slice(&body) {
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
    apply_system_prompt(&mut input.messages, system_prompt);
    let temperature = match input.temperature {
        Some(value) => value,
        None => {
            if assistant.is_some() {
                0.0
            } else {
                1.0
            }
        }
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
    if assistant.is_some() && temperature != 0.0 {
        respond_error(
            request,
            400,
            "speculative decoding currently requires temperature 0; set temperature to 0 or disable it with `lai config speculativeDecoding false`",
        );
        return;
    }
    match log_inference("/v1/chat/completions", model_id, || {
        generate(
            model,
            assistant,
            stream,
            &input.messages,
            temperature,
            max_tokens,
        )
    }) {
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
        Err(_) => {
            respond_error(
                request,
                500,
                "model inference failed; check the server logs",
            );
        }
    }
}

#[cfg(target_os = "macos")]
#[allow(clippy::manual_unwrap_or)] // Defaults are intentional for optional API fields.
fn handle_completion(
    request: tiny_http::Request,
    body: &[u8],
    model_id: &str,
    model: &mut LoadedModel,
    assistant: &mut Option<Gemma4AssistantDraftModel>,
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
        None => {
            if assistant.is_some() {
                0.0
            } else {
                1.0
            }
        }
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
    if assistant.is_some() && temperature != 0.0 {
        respond_error(
            request,
            400,
            "speculative decoding currently requires temperature 0; set temperature to 0 or disable it with `lai config speculativeDecoding false`",
        );
        return;
    }
    match log_inference("/v1/completions", model_id, || {
        generate_text(
            model,
            assistant,
            stream,
            &input.prompt,
            temperature,
            max_tokens,
        )
    }) {
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
        Err(_) => {
            respond_error(
                request,
                500,
                "model inference failed; check the server logs",
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn log_inference(
    endpoint: &str,
    model_id: &str,
    generate: impl FnOnce() -> anyhow::Result<(String, usize, usize, &'static str)>,
) -> anyhow::Result<(String, usize, usize, &'static str)> {
    log::info!("Starting inference for {endpoint} with model {model_id}");
    let started = std::time::Instant::now();
    let result = generate();
    match &result {
        Ok((_, prompt_tokens, completion_tokens, finish)) => log::info!(
            "Finished inference for {endpoint} with model {model_id} in {:.2}s (prompt tokens: {prompt_tokens}, completion tokens: {completion_tokens}, finish: {finish}, output: {:.2} tok/s including prompt processing)",
            started.elapsed().as_secs_f64(),
            InferenceProgress::rate(*completion_tokens, started.elapsed())
        ),
        Err(error) => log::error!(
            "Inference failed for {endpoint} with model {model_id} after {:.2}s: {error:#}",
            started.elapsed().as_secs_f64()
        ),
    }
    result
}

#[cfg(target_os = "macos")]
fn generate(
    model: &mut LoadedModel,
    assistant: &mut Option<Gemma4AssistantDraftModel>,
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
    generate_text(model, assistant, stream, &prompt, temperature, max_tokens)
}

#[cfg(target_os = "macos")]
fn generate_text(
    model: &mut LoadedModel,
    assistant: &mut Option<Gemma4AssistantDraftModel>,
    stream: &safemlx::Stream,
    prompt: &str,
    temperature: f32,
    max_tokens: usize,
) -> anyhow::Result<(String, usize, usize, &'static str)> {
    let started = std::time::Instant::now();
    let mut progress = InferenceProgress::default();
    let mut report_progress = |count| {
        if let Some(rate) = progress.update(count, started.elapsed()) {
            log::info!(
                "Inference progress: {count}/{max_tokens} output tokens ({rate:.2} tok/s including prompt processing)"
            );
        }
    };
    let prompt_ids = model.encode(prompt, false)?;
    let tokens = model.encode_to_array(prompt, false, stream)?;
    let eos = model.eos_token_ids().to_vec();
    if let Some(assistant) = assistant {
        let target = match model.model_mut() {
            Model::Gemma4(target) => target,
            _ => anyhow::bail!("speculative decoding requires a Gemma 4 target"),
        };
        let (generated, stats) = safemlx_lm::gemma4_mtp::generate_gemma4_mtp_with_progress(
            target,
            assistant,
            &tokens,
            &eos,
            max_tokens,
            temperature,
            None,
            stream,
            &mut report_progress,
        )?;
        log::info!(
            "Speculative decoding: accepted {} of {} draft tokens in {} rounds",
            stats.accepted_tokens,
            stats.draft_tokens,
            stats.rounds
        );
        let finish = if generated.len() < max_tokens {
            "stop"
        } else {
            "length"
        };
        return Ok((
            model.decode(&generated, true)?,
            prompt_ids.len(),
            generated.len(),
            finish,
        ));
    }
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
            report_progress(generated.len());
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
