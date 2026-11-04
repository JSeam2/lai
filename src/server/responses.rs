use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug)]
pub struct RequestError {
    pub status: u16,
    pub message: String,
}

impl RequestError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub struct Prepared {
    pub messages: Vec<(String, String)>,
    pub temperature: f32,
    pub max_tokens: usize,
    pub stream: bool,
    pub instructions: Option<String>,
    pub requested_max_tokens: Option<usize>,
    pub requested_temperature: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    model: String,
    input: Value,
    instructions: Option<String>,
    max_output_tokens: Option<usize>,
    temperature: Option<f32>,
    #[serde(default)]
    stream: bool,
    store: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputMessage {
    #[serde(rename = "type")]
    kind: Option<String>,
    role: String,
    content: Value,
    id: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextPart {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    annotations: Option<Vec<Value>>,
    logprobs: Option<Vec<Value>>,
}

pub fn prepare(
    body: &[u8],
    model_id: &str,
    default_prompt: Option<&str>,
    speculative: bool,
) -> Result<Prepared, RequestError> {
    let input: Request = serde_json::from_slice(body)
        .map_err(|error| RequestError::invalid(format!("invalid request: {error}")))?;
    if input.model != model_id {
        return Err(RequestError {
            status: 404,
            message: "requested model is not currently loaded".into(),
        });
    }
    if input.store == Some(true) {
        return Err(RequestError::invalid(
            "stored responses are not supported; set store to false",
        ));
    }
    let temperature = input
        .temperature
        .unwrap_or(if speculative { 0.0 } else { 1.0 });
    let max_tokens = input.max_output_tokens.unwrap_or(256);
    if !(0.0..=2.0).contains(&temperature) || !(1..=4096).contains(&max_tokens) {
        return Err(RequestError::invalid(
            "temperature must be 0–2 and max_output_tokens must be 1–4096",
        ));
    }
    if speculative && temperature != 0.0 {
        return Err(RequestError::invalid(
            "speculative decoding requires temperature 0",
        ));
    }
    let mut messages = match input.input {
        Value::String(text) if !text.is_empty() => vec![("user".into(), text)],
        Value::Array(items) if !items.is_empty() => items
            .into_iter()
            .map(parse_message)
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(RequestError::invalid(
                "input must be a non-empty string or array of text messages",
            ));
        }
    };
    if let Some(instructions) = &input.instructions {
        messages.insert(0, ("system".into(), instructions.clone()));
    } else if !messages.iter().any(|(role, _)| role == "system")
        && let Some(prompt) = default_prompt
    {
        messages.insert(0, ("system".into(), prompt.into()));
    }
    Ok(Prepared {
        messages,
        temperature,
        max_tokens,
        stream: input.stream,
        instructions: input.instructions,
        requested_max_tokens: input.max_output_tokens,
        requested_temperature: input.temperature,
    })
}

fn parse_message(value: Value) -> Result<(String, String), RequestError> {
    let item: InputMessage = serde_json::from_value(value)
        .map_err(|error| RequestError::invalid(format!("invalid input message: {error}")))?;
    if item.kind.as_deref().is_some_and(|kind| kind != "message")
        || item
            .status
            .as_deref()
            .is_some_and(|status| !matches!(status, "completed" | "incomplete"))
    {
        return Err(RequestError::invalid(
            "only completed text messages are supported",
        ));
    }
    let _ = item.id;
    let role = match item.role.as_str() {
        "developer" | "system" => "system",
        "user" => "user",
        "assistant" => "assistant",
        _ => return Err(RequestError::invalid("unsupported input message role")),
    };
    let content = match item.content {
        Value::String(text) => text,
        Value::Array(parts) if !parts.is_empty() => {
            let mut text = String::new();
            for part in parts {
                let part: TextPart = serde_json::from_value(part).map_err(|error| {
                    RequestError::invalid(format!("invalid text content: {error}"))
                })?;
                if !matches!(
                    (role, part.kind.as_str()),
                    ("assistant", "output_text") | ("user" | "system", "input_text")
                ) || part
                    .annotations
                    .as_ref()
                    .is_some_and(|values| !values.is_empty())
                    || part
                        .logprobs
                        .as_ref()
                        .is_some_and(|values| !values.is_empty())
                {
                    return Err(RequestError::invalid(
                        "only unannotated text content is supported",
                    ));
                }
                text.push_str(&part.text);
            }
            text
        }
        _ => return Err(RequestError::invalid("message content must contain text")),
    };
    Ok((role.into(), content))
}

pub struct Generated {
    pub text: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub finish: &'static str,
}

pub fn response(id: &str, model: &str, created: u64, generated: &Generated) -> Value {
    let incomplete = generated.finish == "length";
    json!({
        "id": id, "object": "response", "created_at": created,
        "status": if incomplete { "incomplete" } else { "completed" },
        "error": null,
        "incomplete_details": if incomplete { json!({"reason": "max_output_tokens"}) } else { Value::Null },
        "model": model,
        "output": [{"id": format!("msg_{id}"), "type": "message", "status": if incomplete { "incomplete" } else { "completed" }, "role": "assistant",
            "content": [{"type": "output_text", "text": generated.text, "annotations": [], "logprobs": []}]}],
        "usage": {"input_tokens": generated.input_tokens, "input_tokens_details": {"cached_tokens": 0},
            "output_tokens": generated.output_tokens, "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": generated.input_tokens + generated.output_tokens},
        "parallel_tool_calls": false, "tool_choice": "none", "tools": [], "instructions": null,
        "metadata": {}, "temperature": null, "top_p": null, "max_output_tokens": null,
        "previous_response_id": null, "text": {"format": {"type": "text"}}, "truncation": "disabled"
    })
}

pub fn response_events(result: &Value) -> String {
    let item = &result["output"][0];
    let part = &item["content"][0];
    let item_id = &item["id"];
    let text = &part["text"];
    let mut started = result.clone();
    started["status"] = json!("in_progress");
    started["output"] = json!([]);
    started["usage"] = Value::Null;
    started["incomplete_details"] = Value::Null;
    let mut pending_item = item.clone();
    pending_item["status"] = json!("in_progress");
    pending_item["content"] = json!([]);
    let events = vec![
        ("response.created", json!({"response": started})),
        ("response.in_progress", json!({"response": started})),
        (
            "response.output_item.added",
            json!({"output_index": 0, "item": pending_item}),
        ),
        (
            "response.content_part.added",
            json!({"item_id": item_id, "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []}}),
        ),
        (
            "response.output_text.delta",
            json!({"item_id": item_id, "output_index": 0, "content_index": 0, "delta": text, "logprobs": []}),
        ),
        (
            "response.output_text.done",
            json!({"item_id": item_id, "output_index": 0, "content_index": 0, "text": text, "logprobs": []}),
        ),
        (
            "response.content_part.done",
            json!({"item_id": item_id, "output_index": 0, "content_index": 0, "part": part}),
        ),
        (
            "response.output_item.done",
            json!({"output_index": 0, "item": item}),
        ),
        (
            if result["status"] == "incomplete" {
                "response.incomplete"
            } else {
                "response.completed"
            },
            json!({"response": result}),
        ),
    ];
    let mut body = String::new();
    for (sequence, (name, mut data)) in events.into_iter().enumerate() {
        data["type"] = json!(name);
        data["sequence_number"] = json!(sequence);
        body.push_str(&format!("event: {name}\ndata: {data}\n\n"));
    }
    body
}
