// §5 (a) ships the trait + stub ahead of its consumers. `Role`,
// `Message`, `InferenceRequest`, `InferenceResponse`, `InferenceError`,
// and `infer` are unreachable from prod code paths until §4 (Channel
// surface) wires the trait into a Tauri command and §5 (b) constructs
// the Claude impl that errors. Module-level `#![allow(dead_code)]`
// suppresses warnings for the whole staging area until then.
#![allow(dead_code)]

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
// Provider selection lives in `build_provider()` — §5 (a) always
// returns the stub; §5 (b) reads `ANTHROPIC_API_KEY` and dispatches to
// the Claude impl when it's set, falling back to the stub when it
// isn't (lets the operator run the app for UI-only work without
// burning tokens).

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
// §5 (b) extends this to dispatch to the Claude impl when
// `ANTHROPIC_API_KEY` is set; for §5 (a) it always returns the stub.
pub fn build_provider() -> Box<dyn InferenceProvider> {
    Box::new(StubProvider::new())
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
    // through `async_trait`'s `Pin<Box<Future>>` desugaring.
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
}
