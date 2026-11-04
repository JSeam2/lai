#[path = "../src/server/responses.rs"]
mod responses;

use responses::{Generated, prepare, response, response_events};
use serde_json::{Value, json};

fn prepare_json(
    value: Value,
    default: Option<&str>,
    speculative: bool,
) -> Result<responses::Prepared, responses::RequestError> {
    prepare(
        value.to_string().as_bytes(),
        "owner/model",
        default,
        speculative,
    )
}

#[test]
fn accepts_text_and_multi_turn_input_with_instructions() {
    let request = prepare_json(
        json!({"model":"owner/model", "input":"Hello"}),
        Some("Default"),
        false,
    )
    .unwrap();
    assert_eq!(
        request.messages,
        vec![
            ("system".into(), "Default".into()),
            ("user".into(), "Hello".into())
        ]
    );
    assert_eq!(request.max_tokens, 256);
    let request = prepare_json(
        json!({"model":"owner/model", "instructions":"Be concise", "input":[
        {"role":"user", "content":[{"type":"input_text", "text":"Hello"}]},
        {"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":"Hi"}]},
        {"role":"user", "content":"Again"}
    ], "max_output_tokens":32, "temperature":0, "stream":true, "store":false}),
        Some("Default"),
        false,
    )
    .unwrap();
    assert_eq!(
        request.messages,
        vec![
            ("system".into(), "Be concise".into()),
            ("user".into(), "Hello".into()),
            ("assistant".into(), "Hi".into()),
            ("user".into(), "Again".into())
        ]
    );
    assert_eq!(request.max_tokens, 32);
    assert!(request.stream);
    assert_eq!(request.instructions.as_deref(), Some("Be concise"));
    assert_eq!(request.requested_max_tokens, Some(32));
    assert_eq!(request.requested_temperature, Some(0.0));
}

#[test]
fn accepts_previous_text_output_as_a_new_input_item() {
    let output = response(
        "resp_1",
        "owner/model",
        42,
        &Generated {
            text: "Hello".into(),
            input_tokens: 3,
            output_tokens: 1,
            finish: "stop",
        },
    );
    let request = prepare_json(
        json!({"model":"owner/model", "input": [
            {"role":"user", "content":"Hi"}, output["output"][0],
            {"role":"user", "content":"Go on"}
        ]}),
        None,
        false,
    )
    .unwrap();
    assert_eq!(request.messages[1], ("assistant".into(), "Hello".into()));
}

#[test]
fn rejects_unsupported_features_invalid_limits_and_wrong_model() {
    for request in [
        json!({"model":"owner/model", "input":"hi", "tools":[{"type":"web_search"}]}),
        json!({"model":"owner/model", "input":"hi", "previous_response_id":"resp_old"}),
        json!({"model":"owner/model", "input":"hi", "store":true}),
        json!({"model":"owner/model", "input":[{"role":"user", "content":[{"type":"input_image", "image_url":"https://example.com"}]}]}),
        json!({"model":"owner/model", "input":[]}),
        json!({"model":"owner/model", "input":"hi", "max_output_tokens":4097}),
        json!({"model":"owner/model", "input":"hi", "temperature":2.1}),
    ] {
        assert_eq!(prepare_json(request, None, false).unwrap_err().status, 400);
    }
    let error = prepare_json(json!({"model":"other", "input":"hi"}), None, false).unwrap_err();
    assert_eq!(error.status, 404);
    assert!(error.message.contains("not currently loaded"));
    assert_eq!(
        prepare_json(
            json!({"model":"owner/model", "input":"hi", "temperature":1}),
            None,
            true
        )
        .unwrap_err()
        .status,
        400
    );
    assert_eq!(
        prepare_json(json!({"model":"owner/model", "input":"hi"}), None, true)
            .unwrap()
            .temperature,
        0.0
    );
}

#[test]
fn produces_response_objects_and_ordered_stream_events() {
    let generated = Generated {
        text: "Hello".into(),
        input_tokens: 3,
        output_tokens: 1,
        finish: "stop",
    };
    let result = response("resp_1", "owner/model", 42, &generated);
    assert_eq!(result["object"], "response");
    assert_eq!(result["status"], "completed");
    assert_eq!(result["output"][0]["content"][0]["text"], "Hello");
    assert_eq!(result["usage"]["total_tokens"], 4);
    let events = response_events(&result);
    let parsed: Vec<(&str, Value)> = events
        .trim()
        .split("\n\n")
        .map(|event| {
            let mut lines = event.lines();
            let name = lines.next().unwrap().strip_prefix("event: ").unwrap();
            let value = serde_json::from_str(lines.next().unwrap().strip_prefix("data: ").unwrap())
                .unwrap();
            (name, value)
        })
        .collect();
    assert_eq!(parsed.first().unwrap().0, "response.created");
    assert!(parsed.iter().any(|(name, value)| *name == "response.output_text.delta" && value["delta"] == "Hello"));
    assert_eq!(parsed.last().unwrap().0, "response.completed");
    for (index, (name, value)) in parsed.iter().enumerate() {
        assert_eq!(value["type"], *name);
        assert_eq!(value["sequence_number"], index);
    }
    let truncated = response(
        "resp_2",
        "owner/model",
        42,
        &Generated {
            finish: "length",
            ..generated
        },
    );
    assert_eq!(truncated["status"], "incomplete");
    assert_eq!(
        truncated["incomplete_details"]["reason"],
        "max_output_tokens"
    );
    assert!(response_events(&truncated).contains("event: response.incomplete\n"));
}
