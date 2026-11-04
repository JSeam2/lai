#[path = "../src/server/endpoints.rs"]
mod endpoints;

use endpoints::{Endpoint, route};
use tiny_http::Method;

#[test]
fn routes_supported_methods_and_paths() {
    for (method, path, expected) in [
        (Method::Get, "/v1/models", Endpoint::Models),
        (
            Method::Post,
            "/v1/chat/completions",
            Endpoint::ChatCompletions,
        ),
        (Method::Post, "/v1/completions", Endpoint::Completions),
        (Method::Post, "/v1/responses", Endpoint::Responses),
    ] {
        assert_eq!(route(&method, path), Some(expected));
    }
    assert_eq!(route(&Method::Post, "/v1/models"), None);
    assert_eq!(route(&Method::Get, "/v1/responses"), None);
    assert_eq!(route(&Method::Post, "/v1/unknown"), None);
    assert_eq!(route(&Method::Post, "/v1/responses?x=1"), None);
}
