// Anthropic Claude implementation of `InferenceProvider` (§5 (b)). Sits
// behind the trait shipped in §5 (a); selected by `build_provider` when
// `ANTHROPIC_API_KEY` is set.
//
// The HTTP layer is hand-rolled `reqwest` + `serde` rather than a
// community-maintained Anthropic SDK crate. The surface needed at MVP
// (one POST against /v1/messages, four request fields, content-block
// extraction, four error-status branches) is small enough that an
// unofficial transitive maintenance dep doesn't earn its keep on a
// multi-year-horizon personal app. Re-evaluate if Anthropic ships an
// official Rust SDK or if §6 needs surfaces (streaming, tool use,
// caching) that change the cost calculus.
//
// Bundle layout for outbound requests (per Anthropic Messages API):
//   POST {base_url}/v1/messages
//   Headers:
//     x-api-key: <api_key>
//     anthropic-version: 2023-06-01
//     content-type: application/json
//   Body: { "model", "max_tokens", "system", "messages": [{"role", "content"}] }
//
// Response content is a list of typed blocks (text, tool_use, etc.).
// We extract and concatenate every "text" block in order; non-text
// blocks (tool_use, etc.) are ignored at §5 (b) — tool use is out of
// scope until §6 wires it. Anthropic's Messages API does not currently
// emit interleaved text blocks for plain conversational turns, but the
// concatenation is defense-in-depth for any future response shape that
// splits a single assistant turn across multiple text blocks.
//
// Error mapping (per CLAUDE.md "Resolved during Phase 1 §5"):
//   401 / 403 -> Auth
//   429       -> RateLimited
//   other     -> Provider
//   transport -> Network
// Anthropic's structured error envelope (`{"type": "error", "error":
// {"type": "...", "message": "..."}}`) is parsed when present so the
// message reaches the operator; falls back to "HTTP <status>: <body>"
// when the envelope is missing or malformed.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{InferenceError, InferenceProvider, InferenceRequest, InferenceResponse, Role};

const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const MESSAGES_PATH: &str = "/v1/messages";

// Default token budget for Exile responses. Sized for prose at the
// register `EXILE.md` §1 / §1.5 establishes — measured, not maximalist.
// §4 / §6 may promote this to a per-request override; the abstraction
// layer commits to a working default rather than forcing every consumer
// to plumb a number through.
const DEFAULT_MAX_TOKENS: u32 = 4096;

pub struct ClaudeProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl ClaudeProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    // Test-only constructor that points the client at a local
    // wiremock server. Production callers go through `new`; the
    // `base_url` field is otherwise inaccessible.
    #[cfg(test)]
    pub fn new_with_base_url(api_key: String, model: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
        }
    }
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct WireRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<WireMessage<'a>>,
}

fn role_str(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

#[derive(Deserialize)]
struct WireResponse {
    content: Vec<WireContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireContentBlock {
    Text { text: String },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct WireErrorEnvelope {
    error: WireErrorBody,
}

#[derive(Deserialize)]
struct WireErrorBody {
    message: String,
}

#[async_trait]
impl InferenceProvider for ClaudeProvider {
    async fn infer(
        &self,
        request: InferenceRequest,
    ) -> Result<InferenceResponse, InferenceError> {
        let messages: Vec<WireMessage> = request
            .messages
            .iter()
            .map(|m| WireMessage {
                role: role_str(m.role),
                content: m.content.as_str(),
            })
            .collect();
        let body = WireRequest {
            model: self.model.as_str(),
            max_tokens: DEFAULT_MAX_TOKENS,
            system: request.system_prompt.as_str(),
            messages,
        };

        let url = format!("{}{}", self.base_url, MESSAGES_PATH);
        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| InferenceError::Network(e.to_string()))?;

        let status = response.status();
        if status.is_success() {
            let parsed: WireResponse = response
                .json()
                .await
                .map_err(|e| InferenceError::Provider(format!("response parse: {e}")))?;
            let mut content = String::new();
            for block in parsed.content {
                if let WireContentBlock::Text { text } = block {
                    content.push_str(&text);
                }
            }
            return Ok(InferenceResponse { content });
        }

        let body_text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<WireErrorEnvelope>(&body_text)
            .map(|envelope| envelope.error.message)
            .unwrap_or_else(|_| format!("HTTP {}: {}", status.as_u16(), body_text));

        match status.as_u16() {
            401 | 403 => Err(InferenceError::Auth(message)),
            429 => Err(InferenceError::RateLimited),
            _ => Err(InferenceError::Provider(message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::Message;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider_against(server: &MockServer) -> ClaudeProvider {
        ClaudeProvider::new_with_base_url(
            "test-key".to_string(),
            "claude-opus-4-7".to_string(),
            server.uri(),
        )
    }

    fn happy_response() -> serde_json::Value {
        json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "hello back"}],
            "model": "claude-opus-4-7",
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })
    }

    // Pins the Anthropic Messages API request shape: endpoint, headers,
    // and body schema. Drift in any of these (header name, body field
    // ordering / naming, max_tokens default) breaks the test. This is
    // the load-bearing assertion for §5 (b) — every other test is a
    // branch off this skeleton.
    #[tokio::test]
    async fn happy_path_sends_anthropic_request_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "test-key"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(header("content-type", "application/json"))
            .and(body_json(json!({
                "model": "claude-opus-4-7",
                "max_tokens": 4096,
                "system": "You are Exile.",
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "content": "ready"},
                    {"role": "user", "content": "go"}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(happy_response()))
            .mount(&server)
            .await;

        let provider = provider_against(&server);
        let response = provider
            .infer(InferenceRequest {
                system_prompt: "You are Exile.".into(),
                messages: vec![
                    Message {
                        role: Role::User,
                        content: "hi".into(),
                    },
                    Message {
                        role: Role::Assistant,
                        content: "ready".into(),
                    },
                    Message {
                        role: Role::User,
                        content: "go".into(),
                    },
                ],
            })
            .await
            .unwrap();
        assert_eq!(response.content, "hello back");
    }

    // Defense-in-depth: multi-block responses concatenate text blocks
    // and skip non-text blocks (tool_use is the canonical example,
    // out-of-scope until §6).
    #[tokio::test]
    async fn text_blocks_concatenate_and_non_text_blocks_are_skipped() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "msg_test",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "first."},
                    {"type": "tool_use", "id": "tu_1", "name": "noop", "input": {}},
                    {"type": "text", "text": " second."}
                ],
                "model": "claude-opus-4-7",
                "stop_reason": "end_turn"
            })))
            .mount(&server)
            .await;

        let provider = provider_against(&server);
        let response = provider
            .infer(InferenceRequest {
                system_prompt: String::new(),
                messages: vec![Message {
                    role: Role::User,
                    content: "ping".into(),
                }],
            })
            .await
            .unwrap();
        assert_eq!(response.content, "first. second.");
    }

    #[tokio::test]
    async fn unauthorized_maps_to_auth_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "type": "error",
                "error": {"type": "authentication_error", "message": "invalid x-api-key"}
            })))
            .mount(&server)
            .await;

        let provider = provider_against(&server);
        let err = provider
            .infer(InferenceRequest {
                system_prompt: String::new(),
                messages: vec![Message {
                    role: Role::User,
                    content: "ping".into(),
                }],
            })
            .await
            .unwrap_err();
        match err {
            InferenceError::Auth(message) => {
                assert!(
                    message.contains("invalid x-api-key"),
                    "expected anthropic message, got: {message}"
                );
            }
            other => panic!("expected Auth, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn forbidden_maps_to_auth_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "type": "error",
                "error": {"type": "permission_error", "message": "forbidden"}
            })))
            .mount(&server)
            .await;

        let provider = provider_against(&server);
        let err = provider
            .infer(InferenceRequest {
                system_prompt: String::new(),
                messages: vec![Message {
                    role: Role::User,
                    content: "ping".into(),
                }],
            })
            .await
            .unwrap_err();
        assert!(matches!(err, InferenceError::Auth(_)));
    }

    #[tokio::test]
    async fn rate_limited_maps_to_rate_limited_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "type": "error",
                "error": {"type": "rate_limit_error", "message": "slow down"}
            })))
            .mount(&server)
            .await;

        let provider = provider_against(&server);
        let err = provider
            .infer(InferenceRequest {
                system_prompt: String::new(),
                messages: vec![Message {
                    role: Role::User,
                    content: "ping".into(),
                }],
            })
            .await
            .unwrap_err();
        assert!(matches!(err, InferenceError::RateLimited));
    }

    #[tokio::test]
    async fn server_error_maps_to_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "type": "error",
                "error": {"type": "api_error", "message": "internal"}
            })))
            .mount(&server)
            .await;

        let provider = provider_against(&server);
        let err = provider
            .infer(InferenceRequest {
                system_prompt: String::new(),
                messages: vec![Message {
                    role: Role::User,
                    content: "ping".into(),
                }],
            })
            .await
            .unwrap_err();
        match err {
            InferenceError::Provider(message) => {
                assert!(
                    message.contains("internal"),
                    "expected provider message, got: {message}"
                );
            }
            other => panic!("expected Provider, got: {other:?}"),
        }
    }

    // Non-standard error body (no Anthropic envelope) still maps to
    // Provider on a non-success status, with a graceful fallback
    // message. Guards the unwrap_or_else fallback path.
    #[tokio::test]
    async fn non_envelope_error_body_falls_back_to_status_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(502).set_body_string("upstream broken"))
            .mount(&server)
            .await;

        let provider = provider_against(&server);
        let err = provider
            .infer(InferenceRequest {
                system_prompt: String::new(),
                messages: vec![Message {
                    role: Role::User,
                    content: "ping".into(),
                }],
            })
            .await
            .unwrap_err();
        match err {
            InferenceError::Provider(message) => {
                assert!(message.contains("502"), "expected status code, got: {message}");
                assert!(
                    message.contains("upstream broken"),
                    "expected body in fallback message, got: {message}"
                );
            }
            other => panic!("expected Provider, got: {other:?}"),
        }
    }

    // Live smoke test against the real Anthropic API. Disabled by
    // default (no live API calls in CI). Run with:
    //   $env:ANTHROPIC_API_KEY = "sk-ant-..."
    //   cargo test --manifest-path src-tauri/Cargo.toml --lib \
    //     --ignored live_smoke -- --nocapture
    // Token cost is bounded — small system prompt, single-turn user
    // message, ten-word reply cap in the system prompt. Useful as an
    // end-to-end gut-check that the §5 (b) Claude impl actually
    // round-trips against api.anthropic.com before §4 wires it into
    // the Channel surface.
    #[tokio::test]
    #[ignore = "hits real Anthropic API; run with --ignored and ANTHROPIC_API_KEY set"]
    async fn live_smoke_against_real_api() {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .expect("set ANTHROPIC_API_KEY in the environment to run this smoke test");
        let provider = ClaudeProvider::new(api_key, "claude-opus-4-7".to_string());
        let response = provider
            .infer(InferenceRequest {
                system_prompt: "You are a brief assistant. Reply in ten words or fewer."
                    .into(),
                messages: vec![Message {
                    role: Role::User,
                    content: "Say hello.".into(),
                }],
            })
            .await
            .expect("real API call should succeed");
        eprintln!(
            "\n=== live anthropic response ===\n{}\n================================",
            response.content
        );
        assert!(!response.content.is_empty(), "expected non-empty content");
    }

    // Connection failure (unreachable base URL) maps to Network. Port 1
    // is reserved + unbound on a typical test host, so the connect
    // attempt fails fast.
    #[tokio::test]
    async fn network_failure_maps_to_network_error() {
        let provider = ClaudeProvider::new_with_base_url(
            "test-key".to_string(),
            "claude-opus-4-7".to_string(),
            "http://127.0.0.1:1".to_string(),
        );
        let err = provider
            .infer(InferenceRequest {
                system_prompt: String::new(),
                messages: vec![Message {
                    role: Role::User,
                    content: "ping".into(),
                }],
            })
            .await
            .unwrap_err();
        assert!(matches!(err, InferenceError::Network(_)));
    }
}
