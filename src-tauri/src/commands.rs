// Tauri command surface for the §3 (c) onboarding wizard and unlock screen.
// Thin wrappers over `vault::*` (passphrase / sentinel state machine) and
// `db::put_*` (encrypted-column writes) — the heavy lifting lives in those
// modules so they remain unit-testable without a Tauri runtime.
//
// State model:
//   AppState carries the resolved coo_dir, an open Connection (Mutex'd
//   for cross-command access), and a Mutex<Option<UnlockedVault>>. The
//   vault is None at startup; vault_setup or vault_unlock populates it.
//   The MasterKey never crosses the IPC wire — only command results do.
//
// Lock ordering convention: vault before db. Both write commands extract
// the per-domain key from the vault (releasing the vault lock immediately)
// before taking the db lock, so the two locks are never held simultaneously.
//
// Passphrase ergonomics: arrives from JS as a serde-deserialized String.
// We immediately consume its bytes and let it drop. The String's heap
// allocation is freed but not zeroized — same in-memory hygiene debt that
// `crypto::envelope::decrypt` already carries on plaintext output, tracked
// in CLAUDE.md "Documentary debt to retire".

use crate::crypto::Domain;
use crate::db;
use crate::inference::{self, InferenceProvider};
use crate::prompt;
use crate::vault::{self, UnlockedVault};
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub coo_dir: PathBuf,
    pub db: Mutex<Connection>,
    pub vault: Mutex<Option<UnlockedVault>>,
    // §5 (a) — inference provider as a trait object so §5 (b) can swap
    // in the Claude impl by changing only `inference::build_provider`.
    // No Mutex: the trait is Send + Sync and `&dyn InferenceProvider`
    // is shared across Tauri command threads; concurrent `infer` calls
    // don't conflict at the abstraction layer.
    pub inference: Box<dyn InferenceProvider>,
}

// JSON-serializable mirror of vault::InitState plus the
// onboarding_completed flag from app_config. Bundled into one shape so JS
// makes a single round-trip to decide where to route.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InspectResult {
    Uninitialized,
    Initialized { onboarding_completed: bool },
    Inconsistent { reason: String },
}

const ONBOARDING_COMPLETED_KEY: &str = "onboarding_completed_at";

#[tauri::command]
pub fn vault_inspect(state: State<'_, AppState>) -> Result<InspectResult, String> {
    let init_state = vault::inspect(&state.coo_dir);
    match init_state {
        vault::InitState::Uninitialized => Ok(InspectResult::Uninitialized),
        vault::InitState::Inconsistent(reason) => Ok(InspectResult::Inconsistent {
            reason: reason.to_string(),
        }),
        vault::InitState::Initialized => {
            let conn = state.db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
            let completed: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM app_config WHERE key = ?1)",
                    [ONBOARDING_COMPLETED_KEY],
                    |r| r.get(0),
                )
                .map_err(|e| format!("query app_config: {e}"))?;
            Ok(InspectResult::Initialized {
                onboarding_completed: completed,
            })
        }
    }
}

#[tauri::command]
pub fn vault_setup(passphrase: String, state: State<'_, AppState>) -> Result<(), String> {
    {
        let guard = state.vault.lock().map_err(|e| format!("vault lock poisoned: {e}"))?;
        if guard.is_some() {
            return Err("vault already unlocked".to_string());
        }
    }
    let unlocked = vault::setup_passphrase(&state.coo_dir, passphrase.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut guard = state.vault.lock().map_err(|e| format!("vault lock poisoned: {e}"))?;
    *guard = Some(unlocked);
    Ok(())
}

#[tauri::command]
pub fn vault_unlock(passphrase: String, state: State<'_, AppState>) -> Result<(), String> {
    {
        let guard = state.vault.lock().map_err(|e| format!("vault lock poisoned: {e}"))?;
        if guard.is_some() {
            return Err("vault already unlocked".to_string());
        }
    }
    let unlocked = vault::unlock(&state.coo_dir, passphrase.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut guard = state.vault.lock().map_err(|e| format!("vault lock poisoned: {e}"))?;
    *guard = Some(unlocked);
    Ok(())
}

#[tauri::command]
pub fn write_app_config(
    key: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let conn = state.db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    db::put_app_config(&conn, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_operator_profile(
    plaintext: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let domain_key = {
        let guard = state.vault.lock().map_err(|e| format!("vault lock poisoned: {e}"))?;
        let vault_ref = guard.as_ref().ok_or_else(|| "vault is locked".to_string())?;
        vault_ref.domain_key(Domain::OperatorKnowledge)
    };
    let conn = state.db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    db::put_operator_profile(&conn, &domain_key, plaintext.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn write_calibration_setting(
    dial_key: String,
    plaintext: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let domain_key = {
        let guard = state.vault.lock().map_err(|e| format!("vault lock poisoned: {e}"))?;
        let vault_ref = guard.as_ref().ok_or_else(|| "vault is locked".to_string())?;
        vault_ref.domain_key(Domain::OperatorKnowledge)
    };
    let conn = state.db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
    db::put_calibration_setting(&conn, &domain_key, &dial_key, plaintext.as_bytes())
        .map_err(|e| e.to_string())
}

// JSON-serializable mirror of `inference::InferenceError`. Plain
// `String` would collapse the four variants into one opaque message
// at the IPC boundary; the channel surface needs to render distinct
// UI for "check your API key" (Auth) vs. "wait a moment"
// (RateLimited) vs. "connection failed" (Network) vs. provider
// message (Provider). Tagged enum lets JS pattern-match on `kind`.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InferenceCommandError {
    Auth { message: String },
    Network { message: String },
    RateLimited,
    Provider { message: String },
}

impl From<inference::InferenceError> for InferenceCommandError {
    fn from(err: inference::InferenceError) -> Self {
        match err {
            inference::InferenceError::Auth(message) => Self::Auth { message },
            inference::InferenceError::Network(message) => Self::Network { message },
            inference::InferenceError::RateLimited => Self::RateLimited,
            inference::InferenceError::Provider(message) => Self::Provider { message },
        }
    }
}

// The §4 (a) Channel surface command. Takes the conversation
// turn-list as it stands on the JS side, assembles the system prompt
// from `EXILE.md` §1 + §1.5 + §2 verbatim (per `RAPPORT-STATE-MODEL.md`
// §5.2 step 1 — the load-bearing core only at §4 (a); state-derived
// modifiers and calibration ceiling clamp land in subsequent slices),
// and routes the request through whichever provider
// `inference::build_provider` selected at startup (Claude when
// `ANTHROPIC_API_KEY` is set, stub otherwise).
//
// System prompt assembly is deliberately server-side: JS never sees
// the doctrine text and never has the option to bypass it. The
// `messages` argument carries only the operator/assistant turn
// history.
//
// No state lock taken — `&dyn InferenceProvider` is shared safely
// across Tauri command threads, concurrent `infer` calls don't
// conflict at the abstraction layer (per the §5 (a) AppState shape
// decision in CLAUDE.md "Resolved during Phase 1 §5").
#[tauri::command]
pub async fn infer(
    messages: Vec<inference::Message>,
    state: State<'_, AppState>,
) -> Result<String, InferenceCommandError> {
    let request = inference::InferenceRequest {
        system_prompt: prompt::assemble_system_prompt().to_string(),
        messages,
    };
    let response = state.inference.infer(request).await?;
    Ok(response.content)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The `From<InferenceError>` impl is what translates the inference
    // layer's error model to the IPC layer. Locked here so a future
    // change to either enum's variant set fails loudly rather than
    // silently mapping wrong (e.g. forgetting a new variant after
    // `InferenceError` adds a fifth case).
    #[test]
    fn from_inference_error_preserves_each_variant() {
        let auth = InferenceCommandError::from(inference::InferenceError::Auth("k".into()));
        assert!(matches!(auth, InferenceCommandError::Auth { ref message } if message == "k"));

        let net =
            InferenceCommandError::from(inference::InferenceError::Network("down".into()));
        assert!(matches!(net, InferenceCommandError::Network { ref message } if message == "down"));

        let rl = InferenceCommandError::from(inference::InferenceError::RateLimited);
        assert!(matches!(rl, InferenceCommandError::RateLimited));

        let prov = InferenceCommandError::from(inference::InferenceError::Provider("oops".into()));
        assert!(matches!(prov, InferenceCommandError::Provider { ref message } if message == "oops"));
    }

    // Wire-shape KAT for the four variants. JS pattern-matches on
    // `kind` and reads `message` for the three message-bearing
    // variants. Drift in the field name, the tag name, or the
    // snake_case transform breaks the channel surface's error
    // rendering — this test makes that drift loud.
    #[test]
    fn inference_command_error_wire_shape_is_pinned() {
        use serde_json::{json, to_value};

        assert_eq!(
            to_value(InferenceCommandError::Auth { message: "x".into() }).unwrap(),
            json!({ "kind": "auth", "message": "x" })
        );
        assert_eq!(
            to_value(InferenceCommandError::Network { message: "x".into() }).unwrap(),
            json!({ "kind": "network", "message": "x" })
        );
        assert_eq!(
            to_value(InferenceCommandError::RateLimited).unwrap(),
            json!({ "kind": "rate_limited" })
        );
        assert_eq!(
            to_value(InferenceCommandError::Provider { message: "x".into() }).unwrap(),
            json!({ "kind": "provider", "message": "x" })
        );
    }
}
