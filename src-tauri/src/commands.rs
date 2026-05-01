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
use crate::db::{self, DecryptedTurn, TurnRole};
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
//
// At §4 (b) the rewired `infer` command performs conversation-table
// writes before and after the inference call. db / crypto failures
// inside `infer` map to `Provider("conversation persistence: ...")`
// rather than extending this enum — preserves the §4 (a1) wire-shape
// KAT (`inference_command_error_wire_shape_is_pinned`) and gives the
// React surface a single passthrough rendering path. The trade-off:
// the React side cannot distinguish a transient network error from a
// db write failure, but in §4 (b)'s scope the on-disk failure modes
// are vanishingly rare (encrypted-write of a UTF-8 string under a
// derived domain key) and the operator-actionable response is the
// same: try again or surface to the implementer if it persists.
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

impl From<db::DbError> for InferenceCommandError {
    fn from(err: db::DbError) -> Self {
        // Db / crypto failures inside `infer` collapse into Provider so
        // the §4 (a1) wire shape stays unchanged. The underlying error
        // text is preserved so the surface can render an actionable
        // message; the variant is the same the operator already knows
        // means "the upstream side of the call did not succeed."
        Self::Provider {
            message: format!("conversation persistence: {err}"),
        }
    }
}

// JSON-serializable mirror of conversation-flow errors for the new
// `load_conversation` and `append_turn` commands. Distinct from
// `InferenceCommandError` because these commands do not call the
// inference provider — they touch the vault and the db only — and the
// React surface should distinguish "vault not unlocked yet" from
// "inference request failed."
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConversationCommandError {
    VaultLocked,
    Db { message: String },
    Crypto { message: String },
}

impl From<db::DbError> for ConversationCommandError {
    fn from(err: db::DbError) -> Self {
        match err {
            db::DbError::Crypto(c) => Self::Crypto {
                message: c.to_string(),
            },
            other => Self::Db {
                message: other.to_string(),
            },
        }
    }
}

// JSON-serializable view of one turn returned to the React side for
// scrollback rendering. `turn_index` is the disk-assigned ordinal;
// React uses it as the React key. `created_at` is the plaintext ISO
// timestamp (UTC, millisecond precision, `Z` suffix) used by the
// date-divider rendering.
#[derive(Debug, Serialize)]
pub struct TurnPayload {
    pub turn_index: i64,
    pub role: TurnRole,
    pub content: String,
    pub created_at: String,
}

impl From<DecryptedTurn> for TurnPayload {
    fn from(turn: DecryptedTurn) -> Self {
        Self {
            turn_index: turn.turn_index,
            role: turn.role,
            content: turn.content,
            created_at: turn.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LoadConversationResponse {
    pub session_id: String,
    pub turns: Vec<TurnPayload>,
}

// Returned by the rewired `infer` command. Both turn indices and both
// timestamps are surfaced so the React side can replace its optimistic
// turn (`turn_index = -1`, locally generated `created_at`) with the
// disk-authoritative values in one round-trip.
#[derive(Debug, Serialize)]
pub struct TurnIndices {
    pub user: i64,
    pub assistant: i64,
}

#[derive(Debug, Serialize)]
pub struct TurnTimestamps {
    pub user: String,
    pub assistant: String,
}

#[derive(Debug, Serialize)]
pub struct InferResponse {
    pub assistant_content: String,
    pub turn_indices: TurnIndices,
    pub created_at: TurnTimestamps,
}

// Translate the db layer's `TurnRole` into the inference layer's
// `Role`. Layering: inference → db is fine; db → inference would be
// an inversion, so the db layer carries its own `TurnRole` enum and
// the conversion happens at the command boundary where both layers
// are already in scope.
fn turn_role_to_inference(role: TurnRole) -> inference::Role {
    match role {
        TurnRole::User => inference::Role::User,
        TurnRole::Assistant => inference::Role::Assistant,
    }
}

// Find or create the session the React side should be working in.
// Helper used by both `load_conversation` and `append_turn` so the
// "first launch with no sessions yet" path is handled uniformly.
fn ensure_session(conn: &Connection) -> Result<String, db::DbError> {
    if let Some(id) = db::latest_session_id(conn)? {
        return Ok(id);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = db::current_iso_timestamp(conn)?;
    db::create_session(conn, &id, &now)?;
    Ok(id)
}

#[tauri::command]
pub fn load_conversation(
    state: State<'_, AppState>,
) -> Result<LoadConversationResponse, ConversationCommandError> {
    let domain_key = {
        let guard = state
            .vault
            .lock()
            .map_err(|e| ConversationCommandError::Db {
                message: format!("vault lock poisoned: {e}"),
            })?;
        let vault_ref = guard
            .as_ref()
            .ok_or(ConversationCommandError::VaultLocked)?;
        vault_ref.domain_key(Domain::Conversation)
    };
    let conn = state.db.lock().map_err(|e| ConversationCommandError::Db {
        message: format!("db lock poisoned: {e}"),
    })?;
    let session_id = ensure_session(&conn)?;
    let decrypted = db::list_turns_for_ui(&conn, &domain_key, &session_id)?;
    let turns = decrypted.into_iter().map(TurnPayload::from).collect();
    Ok(LoadConversationResponse { session_id, turns })
}

// Surfaced for completeness — not consumed by the §4 (b) React flow,
// which writes turns indirectly through `infer`. Available so a
// future surface (e.g. a manual test harness or a Phase 2 dobackbone-
// driven entry point) can append a turn without going through the
// inference provider.
#[derive(Debug, Serialize)]
pub struct AppendTurnResponse {
    pub turn_index: i64,
    pub created_at: String,
}

#[tauri::command]
pub fn append_turn(
    session_id: String,
    role: TurnRole,
    content: String,
    state: State<'_, AppState>,
) -> Result<AppendTurnResponse, ConversationCommandError> {
    let domain_key = {
        let guard = state
            .vault
            .lock()
            .map_err(|e| ConversationCommandError::Db {
                message: format!("vault lock poisoned: {e}"),
            })?;
        let vault_ref = guard
            .as_ref()
            .ok_or(ConversationCommandError::VaultLocked)?;
        vault_ref.domain_key(Domain::Conversation)
    };
    let mut conn = state.db.lock().map_err(|e| ConversationCommandError::Db {
        message: format!("db lock poisoned: {e}"),
    })?;
    let tx = conn.transaction().map_err(db::DbError::from)?;
    let turn_index = db::next_turn_index(&tx, &session_id)?;
    let created_at = db::current_iso_timestamp(&tx)?;
    db::put_turn(
        &tx,
        &domain_key,
        &session_id,
        turn_index,
        role,
        &content,
        &created_at,
    )?;
    db::increment_session_turn_count(&tx, &session_id)?;
    tx.commit().map_err(db::DbError::from)?;
    Ok(AppendTurnResponse {
        turn_index,
        created_at,
    })
}

// The §4 (b) Channel surface command. Disk is the source of truth: the
// command appends the operator turn, reads the in-window history (now
// including the turn just written), assembles the system prompt from
// `EXILE.md` §1 + §1.5 + §2 verbatim plus the §4 (a3) output discipline
// directive (per `RAPPORT-STATE-MODEL.md` §5.2 step 1 + §5.5; state-
// derived modifiers and calibration ceiling clamp land in subsequent
// slices), routes the request through whichever provider
// `inference::build_provider` selected at startup, and writes the
// assistant response back to disk.
//
// Lock ordering preserved across the longer flow: vault before db,
// never simultaneously held. The inference call runs with no locks
// held — that's a network round-trip, not something the locks should
// span. The vault is re-acquired on the assistant-write side because
// a future "lock now" affordance could in principle close the vault
// between the operator-write and the assistant-write; the re-derive
// is cheap (HKDF-SHA256 against the master key already in memory).
//
// Lossy mapping: db / crypto failures collapse into
// `InferenceCommandError::Provider("conversation persistence: ...")`
// per the From<DbError> impl above, keeping the §4 (a1) wire shape
// stable.
#[tauri::command]
pub async fn infer(
    session_id: String,
    operator_turn: String,
    state: State<'_, AppState>,
) -> Result<InferResponse, InferenceCommandError> {
    // Step 1 — derive the conversation domain key.
    let domain_key = {
        let guard = state
            .vault
            .lock()
            .map_err(|e| InferenceCommandError::Provider {
                message: format!("vault lock poisoned: {e}"),
            })?;
        let vault_ref = guard
            .as_ref()
            .ok_or_else(|| InferenceCommandError::Provider {
                message: "vault is locked".to_string(),
            })?;
        vault_ref.domain_key(Domain::Conversation)
    };

    // Step 2 — append the operator turn and read the in-window
    // history under a single db lock + transaction. The transaction
    // wraps both writes (turn INSERT + session turn_count UPDATE) so
    // a half-write can't drift the count from the actual rows; the
    // history read happens before commit so it sees the just-written
    // turn at the same isolation snapshot.
    let (user_turn_index, user_created_at, in_window) = {
        let mut conn_guard =
            state.db.lock().map_err(|e| InferenceCommandError::Provider {
                message: format!("db lock poisoned: {e}"),
            })?;
        let tx = conn_guard.transaction().map_err(db::DbError::from)?;
        let turn_index = db::next_turn_index(&tx, &session_id)?;
        let created_at = db::current_iso_timestamp(&tx)?;
        db::put_turn(
            &tx,
            &domain_key,
            &session_id,
            turn_index,
            TurnRole::User,
            &operator_turn,
            &created_at,
        )?;
        db::increment_session_turn_count(&tx, &session_id)?;
        let in_window = db::list_turns_for_inference(&tx, &domain_key, &session_id)?;
        tx.commit().map_err(db::DbError::from)?;
        (turn_index, created_at, in_window)
    };

    // Step 3 — build the inference request from the in-window history
    // and call the provider with no locks held.
    let messages: Vec<inference::Message> = in_window
        .into_iter()
        .map(|t| inference::Message {
            role: turn_role_to_inference(t.role),
            content: t.content,
        })
        .collect();
    let request = inference::InferenceRequest {
        system_prompt: prompt::assemble_system_prompt().to_string(),
        messages,
    };
    let response = state.inference.infer(request).await?;

    // Step 4 — re-derive the conversation key (defense in depth) and
    // write the assistant turn under a fresh transaction.
    let domain_key = {
        let guard = state
            .vault
            .lock()
            .map_err(|e| InferenceCommandError::Provider {
                message: format!("vault lock poisoned: {e}"),
            })?;
        let vault_ref = guard
            .as_ref()
            .ok_or_else(|| InferenceCommandError::Provider {
                message: "vault is locked".to_string(),
            })?;
        vault_ref.domain_key(Domain::Conversation)
    };

    let (assistant_turn_index, assistant_created_at) = {
        let mut conn_guard =
            state.db.lock().map_err(|e| InferenceCommandError::Provider {
                message: format!("db lock poisoned: {e}"),
            })?;
        let tx = conn_guard.transaction().map_err(db::DbError::from)?;
        let turn_index = db::next_turn_index(&tx, &session_id)?;
        let created_at = db::current_iso_timestamp(&tx)?;
        db::put_turn(
            &tx,
            &domain_key,
            &session_id,
            turn_index,
            TurnRole::Assistant,
            &response.content,
            &created_at,
        )?;
        db::increment_session_turn_count(&tx, &session_id)?;
        tx.commit().map_err(db::DbError::from)?;
        (turn_index, created_at)
    };

    Ok(InferResponse {
        assistant_content: response.content,
        turn_indices: TurnIndices {
            user: user_turn_index,
            assistant: assistant_turn_index,
        },
        created_at: TurnTimestamps {
            user: user_created_at,
            assistant: assistant_created_at,
        },
    })
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

    // Wire-shape KAT for the §4 (b) ConversationCommandError. Same
    // tagged-enum pattern as InferenceCommandError; the React side
    // pattern-matches on `kind` and renders distinct UI per variant.
    // VaultLocked carries no payload — operator should never see it
    // in normal flow (App.tsx routes through UnlockScreen first), so
    // a generic message is enough.
    #[test]
    fn conversation_command_error_wire_shape_is_pinned() {
        use serde_json::{json, to_value};

        assert_eq!(
            to_value(ConversationCommandError::VaultLocked).unwrap(),
            json!({ "kind": "vault_locked" })
        );
        assert_eq!(
            to_value(ConversationCommandError::Db { message: "x".into() }).unwrap(),
            json!({ "kind": "db", "message": "x" })
        );
        assert_eq!(
            to_value(ConversationCommandError::Crypto { message: "x".into() }).unwrap(),
            json!({ "kind": "crypto", "message": "x" })
        );
    }

    // Wire-shape KAT for the §4 (b) InferResponse. The React side
    // depends on this shape to splice the disk-authoritative turn
    // indices and timestamps into its scrollback after each round-
    // trip. Drift in any field name (assistant_content, turn_indices,
    // created_at) or in the nested struct shapes (user/assistant
    // pairs) breaks the channel surface's optimistic-replace path.
    #[test]
    fn infer_response_wire_shape_is_pinned() {
        use serde_json::{json, to_value};

        let response = InferResponse {
            assistant_content: "yes.".into(),
            turn_indices: TurnIndices {
                user: 4,
                assistant: 5,
            },
            created_at: TurnTimestamps {
                user: "2026-05-01T12:00:01.000Z".into(),
                assistant: "2026-05-01T12:00:02.000Z".into(),
            },
        };
        assert_eq!(
            to_value(response).unwrap(),
            json!({
                "assistant_content": "yes.",
                "turn_indices": { "user": 4, "assistant": 5 },
                "created_at": {
                    "user": "2026-05-01T12:00:01.000Z",
                    "assistant": "2026-05-01T12:00:02.000Z"
                }
            })
        );
    }
}
