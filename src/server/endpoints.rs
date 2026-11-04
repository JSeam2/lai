use tiny_http::Method;

#[derive(Debug, PartialEq, Eq)]
pub enum Endpoint {
    Models,
    ChatCompletions,
    Completions,
    Responses,
}

/// The HTTP methods and paths supported by the local server.
pub fn route(method: &Method, path: &str) -> Option<Endpoint> {
    match (method, path) {
        (&Method::Get, "/v1/models") => Some(Endpoint::Models),
        (&Method::Post, "/v1/chat/completions") => Some(Endpoint::ChatCompletions),
        (&Method::Post, "/v1/completions") => Some(Endpoint::Completions),
        (&Method::Post, "/v1/responses") => Some(Endpoint::Responses),
        _ => None,
    }
}
