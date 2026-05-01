// Inference provider abstraction per ADR-0011 and `mvp/coo.md` §5 —
// the seam that lets COO swap providers (and eventually local
// inference) without an application rewrite. §5 (a) ships the trait +
// the stub provider; §5 (b) plugs the Anthropic Claude implementation
// in behind it; §4 (Channel surface) consumes the trait, never a
// concrete impl.
//
// Trait shape decisions (per CLAUDE.md "Resolved during Phase 1 §5"):
//
//   - Non-streaming `infer` only at §5 (a). Streaming (`infer_stream`)
//     is an additive future addition: a default-implemented method on
//     this trait, overridden by the Claude impl when §4 needs it.
//     Adding a default-implemented method to a trait does not break
//     existing implementors.
//
//   - `async_trait` macro for trait-object dispatch. Native async fn
//     in traits (Rust 1.75+) doesn't yet allow `Box<dyn Trait>` for
//     async trait methods; `async_trait`'s `Pin<Box<Future>>`
//     desugaring does. The Pin<Box> indirection is irrelevant for
//     network-bound inference calls.
//
//   - Trait is `Send + Sync` so the resulting `Box<dyn
//     InferenceProvider>` lives in `tauri::State` and can be shared
//     across Tauri command threads.
//
// Provider selection lives in `build_provider()` — §5 (b) reads
// `ANTHROPIC_API_KEY` and dispatches to the Claude impl when it's set,
// falling back to the stub when it isn't (lets the operator run the
// app for UI-only work without burning tokens).

mod claude;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

// The system prompt is held separate from `messages` because Claude's
// API treats it that way and because `RAPPORT-STATE-MODEL.md` §5.2's
// inference assembly pipeline produces it as a distinct artifact
// (EXILE.md §1 + §1.5 + §2 verbatim, state-derived prose modifiers,
// calibration ceiling clamp, wellbeing posture). Putting it in
// `messages` would mean every consumer reconstructs the split, which
// invites bugs.
#[derive(Clone, Debug)]
pub struct InferenceRequest {
    pub system_prompt: String,
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug)]
pub struct InferenceResponse {
    pub content: String,
}

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    // Provider returned a structured error we cannot more specifically
    // classify (model overloaded, content filter, etc.).
    #[error("provider error: {0}")]
    Provider(String),
    #[error("network error: {0}")]
    Network(String),
    // 401/403 from the API. Most likely a missing or invalid
    // `ANTHROPIC_API_KEY`.
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("rate limited")]
    RateLimited,
}

#[async_trait]
pub trait InferenceProvider: Send + Sync {
    async fn infer(
        &self,
        request: InferenceRequest,
    ) -> Result<InferenceResponse, InferenceError>;
}

// Stub provider. Echoes the last user message back with a `[stub]`
// prefix so §4 development can verify the system prompt and message
// history are reaching the provider correctly without burning API
// tokens, and so KATs against the inference seam stay deterministic.
pub struct StubProvider;

impl StubProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl InferenceProvider for StubProvider {
    async fn infer(
        &self,
        request: InferenceRequest,
    ) -> Result<InferenceResponse, InferenceError> {
        let last_user = request
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
            .unwrap_or("(no user message)");
        Ok(InferenceResponse {
            content: format!("[stub] you said: {}", last_user),
        })
    }
}

// Constructor used at startup to populate `AppState.inference`.
// `ANTHROPIC_API_KEY` set + non-empty → ClaudeProvider; otherwise the
// stub. `COO_INFERENCE_MODEL` overrides the default model id when set
// + non-empty; default is `claude-opus-4-7`.
pub fn build_provider() -> Box<dyn InferenceProvider> {
    let api_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let model_override = std::env::var("COO_INFERENCE_MODEL").ok();
    build_provider_from_env(api_key, model_override)
}

// Pure dispatch helper: the env-var read is in `build_provider`; this
// function is testable without touching process env.
fn build_provider_from_env(
    api_key: Option<String>,
    model_override: Option<String>,
) -> Box<dyn InferenceProvider> {
    let api_key = api_key.filter(|s| !s.is_empty());
    let model = model_override
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "claude-opus-4-7".to_string());
    match api_key {
        Some(key) => Box::new(claude::ClaudeProvider::new(key, model)),
        None => Box::new(StubProvider::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(messages: Vec<Message>) -> InferenceRequest {
        InferenceRequest {
            system_prompt: String::new(),
            messages,
        }
    }

    #[tokio::test]
    async fn stub_echoes_last_user_message() {
        let provider = StubProvider::new();
        let response = provider
            .infer(req(vec![
                Message {
                    role: Role::User,
                    content: "hello".into(),
                },
                Message {
                    role: Role::Assistant,
                    content: "hi".into(),
                },
                Message {
                    role: Role::User,
                    content: "how are you".into(),
                },
            ]))
            .await
            .unwrap();
        assert_eq!(response.content, "[stub] you said: how are you");
    }

    #[tokio::test]
    async fn stub_handles_no_user_messages() {
        let provider = StubProvider::new();
        let response = provider.infer(req(vec![])).await.unwrap();
        assert_eq!(response.content, "[stub] you said: (no user message)");
    }

    #[tokio::test]
    async fn stub_skips_assistant_only_history() {
        let provider = StubProvider::new();
        let response = provider
            .infer(req(vec![Message {
                role: Role::Assistant,
                content: "monologue".into(),
            }]))
            .await
            .unwrap();
        assert_eq!(response.content, "[stub] you said: (no user message)");
    }

    // Proves the trait can be erased to a `Box<dyn InferenceProvider>`
    // (the shape `AppState` will hold it as) and still dispatch
    // through `async_trait`'s `Pin<Box<Future>>` desugaring. Also
    // covers the env-unset path of `build_provider` indirectly: in the
    // test environment `ANTHROPIC_API_KEY` is unset, so the stub is
    // selected and the `[stub]` prefix proves the dispatch.
    #[tokio::test]
    async fn build_provider_returns_a_working_trait_object() {
        let provider: Box<dyn InferenceProvider> = build_provider();
        let response = provider
            .infer(req(vec![Message {
                role: Role::User,
                content: "ping".into(),
            }]))
            .await
            .unwrap();
        assert_eq!(response.content, "[stub] you said: ping");
    }

    #[tokio::test]
    async fn build_provider_from_env_falls_back_to_stub_when_key_absent() {
        let provider = build_provider_from_env(None, None);
        let response = provider
            .infer(req(vec![Message {
                role: Role::User,
                content: "ping".into(),
            }]))
            .await
            .unwrap();
        assert_eq!(response.content, "[stub] you said: ping");
    }

    // Empty-string key is treated identically to unset so the operator
    // can `unset ANTHROPIC_API_KEY` or `export ANTHROPIC_API_KEY=`
    // without surprises.
    #[tokio::test]
    async fn build_provider_from_env_falls_back_to_stub_when_key_empty() {
        let provider = build_provider_from_env(Some(String::new()), None);
        let response = provider
            .infer(req(vec![Message {
                role: Role::User,
                content: "ping".into(),
            }]))
            .await
            .unwrap();
        assert_eq!(response.content, "[stub] you said: ping");
    }

    // Empty-string model override falls back to the default rather
    // than passing the empty string to the Claude impl as a literal
    // model id (which would error at the API layer).
    #[tokio::test]
    async fn build_provider_from_env_treats_empty_model_override_as_unset() {
        let provider = build_provider_from_env(None, Some(String::new()));
        let response = provider
            .infer(req(vec![Message {
                role: Role::User,
                content: "ping".into(),
            }]))
            .await
            .unwrap();
        assert_eq!(response.content, "[stub] you said: ping");
    }
}
