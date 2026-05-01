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
use crate::db::{self, DecryptedSummary, DecryptedTurn, SessionBoundary, SummaryKind, TurnRole};
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

// §4 (c) — surfaced summary covering a turn range. Rendered by the
// React `<SummaryStanza>` component in place of the turns it covers.
// `kind` and `session_id` are exposed so a future operator-tooling
// surface can distinguish within-session from cross-session
// summaries without an extra round-trip.
#[derive(Debug, Serialize)]
pub struct SummaryPayload {
    pub session_id: String,
    pub kind: SummaryKind,
    pub covers_turn_range_start: i64,
    pub covers_turn_range_end: i64,
    pub content: String,
    pub generated_at: String,
}

impl From<DecryptedSummary> for SummaryPayload {
    fn from(s: DecryptedSummary) -> Self {
        Self {
            session_id: s.session_id,
            kind: s.kind,
            covers_turn_range_start: s.covers_turn_range_start,
            covers_turn_range_end: s.covers_turn_range_end,
            content: s.content,
            generated_at: s.generated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LoadConversationResponse {
    pub session_id: String,
    pub turns: Vec<TurnPayload>,
    // §4 (c) — summaries (cross-session from prior sessions plus
    // within-session for the current session) for the React side to
    // render in place of covered turns.
    pub summaries: Vec<SummaryPayload>,
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
    // §4 (c) — the (possibly new) session_id the operator turn was
    // written into. When the inactivity-gap boundary fires inside
    // `infer`, the previous session is finalized and a new session
    // is created; this field carries the new id back so the React
    // side stays in sync without an extra `load_conversation`
    // round-trip. When no boundary fires, this is the same id the
    // caller passed in.
    pub session_id: String,
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
    let decrypted_turns = db::list_turns_for_ui(&conn, &domain_key, &session_id)?;
    let turns = decrypted_turns
        .into_iter()
        .map(TurnPayload::from)
        .collect();
    let decrypted_summaries =
        db::list_summaries_for_inference(&conn, &domain_key, &session_id)?;
    let summaries = decrypted_summaries
        .into_iter()
        .map(SummaryPayload::from)
        .collect();
    Ok(LoadConversationResponse {
        session_id,
        turns,
        summaries,
    })
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

// Pending summarization batch captured during the locked phase of
// `infer_impl` and processed (network call) outside the locks.
struct PendingSummary {
    session_id: String,
    kind: SummaryKind,
    range: (i64, i64),
    turns: Vec<DecryptedTurn>,
    // For cross-session summaries, the timestamp recorded as
    // `generated_at` is the prior session's `ended_at` — the moment
    // the boundary fired. For within-session, generated_at is set at
    // write time inside the second locked phase.
    generated_at_override: Option<String>,
}

// Helper: derive the conversation domain key from the vault. Called
// at every lock-reacquisition point in the infer flow because a
// future "lock now" affordance could in principle close the vault
// between phases; the re-derive is cheap HKDF-SHA256 over a 32-byte
// key already in memory.
fn derive_conversation_key(
    vault_mutex: &Mutex<Option<UnlockedVault>>,
) -> Result<crate::crypto::DomainKey, InferenceCommandError> {
    let guard = vault_mutex
        .lock()
        .map_err(|e| InferenceCommandError::Provider {
            message: format!("vault lock poisoned: {e}"),
        })?;
    let vault_ref = guard
        .as_ref()
        .ok_or_else(|| InferenceCommandError::Provider {
            message: "vault is locked".to_string(),
        })?;
    Ok(vault_ref.domain_key(Domain::Conversation))
}

fn lock_db(
    db_mutex: &Mutex<Connection>,
) -> Result<std::sync::MutexGuard<'_, Connection>, InferenceCommandError> {
    db_mutex.lock().map_err(|e| InferenceCommandError::Provider {
        message: format!("db lock poisoned: {e}"),
    })
}

// Run one summarization inference call. The summarization prompt
// holds the character text + output discipline + summarization
// directive + the turns to summarize; the messages list carries a
// single trigger user message (Claude's API requires at least one).
async fn run_summarization_call(
    provider: &dyn InferenceProvider,
    turns: &[DecryptedTurn],
) -> Result<String, InferenceCommandError> {
    let request = inference::InferenceRequest {
        system_prompt: prompt::assemble_summarization_prompt(turns),
        messages: vec![inference::Message {
            role: inference::Role::User,
            content: "Write your recollection now, per the summarization task above.".into(),
        }],
    };
    let response = provider.infer(request).await?;
    Ok(response.content)
}

// Format the prepended summaries as a single synthetic assistant
// message body. Each summary becomes an "Earlier: ..." stanza
// separated by blank lines, in chronological (generated_at) order.
// Empty input returns an empty string — caller checks for that
// before adding the synthetic message.
fn format_summaries_as_synthetic_assistant(summaries: &[DecryptedSummary]) -> String {
    let mut out = String::new();
    for (i, s) in summaries.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str("Earlier: ");
        out.push_str(&s.content);
    }
    out
}

// The Tauri command — thin wrapper around `infer_impl` that pulls
// the underlying primitives off `AppState`. The split exists so
// integration tests (`infer_command_*`) can call `infer_impl`
// directly without a Tauri runtime.
#[tauri::command]
pub async fn infer(
    session_id: String,
    operator_turn: String,
    state: State<'_, AppState>,
) -> Result<InferResponse, InferenceCommandError> {
    infer_impl(
        &state.db,
        &state.vault,
        state.inference.as_ref(),
        session_id,
        operator_turn,
    )
    .await
}

// The §4 (b) + §4 (c) Channel surface flow.
//
// Phases (lock acquisition is bounded; network calls run with no
// locks held):
//
//   Phase 1 (locked) — detect cross-session boundary; if it fires,
//     finalize the old session, capture its unsummarized turns as a
//     pending cross_session summary, and create a new session. Append
//     the operator turn into the active session. Check the within-
//     session threshold; if exceeded, capture the oldest
//     SUMMARIZATION_BATCH_SIZE turns as a pending within_session
//     summary.
//
//   Phase 2 (no locks) — run pending summarization inference calls.
//     Cross-session first (the boundary already fired), then within-
//     session. Both calls use `assemble_summarization_prompt` so
//     Exile summarizes in her own register per RAPPORT-STATE-MODEL.md
//     §4.2.
//
//   Phase 3 (locked) — write summary rows. Read in-window turns +
//     summaries (now including any just-written ones). Commit.
//
//   Phase 4 (no locks) — operator-facing inference call. Messages
//     list = [synthetic assistant with summaries, ...in_window turns].
//
//   Phase 5 (locked) — write the assistant turn.
//
// The cross-session summary's `generated_at` is the prior session's
// `ended_at` — the moment the boundary fired. The within-session
// summary's `generated_at` is the current time at write.
async fn infer_impl(
    db_mutex: &Mutex<Connection>,
    vault_mutex: &Mutex<Option<UnlockedVault>>,
    provider: &dyn InferenceProvider,
    session_id_in: String,
    operator_turn: String,
) -> Result<InferResponse, InferenceCommandError> {
    let active_session_id: String;
    let user_turn_index: i64;
    let user_created_at: String;
    let mut pending: Vec<PendingSummary> = Vec::new();

    // Phase 1.
    {
        let domain_key = derive_conversation_key(vault_mutex)?;
        let mut conn = lock_db(db_mutex)?;
        let tx = conn.transaction().map_err(db::DbError::from)?;

        let now = db::current_iso_timestamp(&tx)?;
        match db::detect_session_boundary(&tx, &session_id_in, &now)? {
            SessionBoundary::Continue => {
                active_session_id = session_id_in.clone();
            }
            SessionBoundary::NewSessionRequired { previous_ended_at } => {
                let unsum = db::unsummarized_range_for_session(&tx, &session_id_in)?;
                let turns_to_summarize = if let Some(range) = unsum {
                    db::list_turns_in_range(
                        &tx,
                        &domain_key,
                        &session_id_in,
                        range.from_turn_index,
                        range.to_turn_index,
                    )?
                } else {
                    Vec::new()
                };
                let new_session_id = uuid::Uuid::new_v4().to_string();
                db::finalize_session(&tx, &session_id_in, &previous_ended_at)?;
                db::create_session(&tx, &new_session_id, &now)?;
                if !turns_to_summarize.is_empty() {
                    let range_start = turns_to_summarize.first().unwrap().turn_index;
                    let range_end = turns_to_summarize.last().unwrap().turn_index;
                    pending.push(PendingSummary {
                        session_id: session_id_in.clone(),
                        kind: SummaryKind::CrossSession,
                        range: (range_start, range_end),
                        turns: turns_to_summarize,
                        generated_at_override: Some(previous_ended_at),
                    });
                }
                active_session_id = new_session_id;
            }
        }

        user_turn_index = db::next_turn_index(&tx, &active_session_id)?;
        user_created_at = db::current_iso_timestamp(&tx)?;
        db::put_turn(
            &tx,
            &domain_key,
            &active_session_id,
            user_turn_index,
            TurnRole::User,
            &operator_turn,
            &user_created_at,
        )?;
        db::increment_session_turn_count(&tx, &active_session_id)?;

        let unsum_count = db::unsummarized_turn_count(&tx, &active_session_id)?;
        let threshold =
            (db::INFERENCE_WINDOW_TURNS + db::SUMMARIZATION_BATCH_SIZE) as i64;
        if unsum_count > threshold {
            let unsum = db::unsummarized_range_for_session(&tx, &active_session_id)?
                .expect("unsummarized count > threshold implies range present");
            let batch_end = unsum.from_turn_index + db::SUMMARIZATION_BATCH_SIZE as i64 - 1;
            let turns_to_summarize = db::list_turns_in_range(
                &tx,
                &domain_key,
                &active_session_id,
                unsum.from_turn_index,
                batch_end,
            )?;
            pending.push(PendingSummary {
                session_id: active_session_id.clone(),
                kind: SummaryKind::WithinSession,
                range: (unsum.from_turn_index, batch_end),
                turns: turns_to_summarize,
                generated_at_override: None,
            });
        }

        tx.commit().map_err(db::DbError::from)?;
    }

    // Phase 2 — run pending summarization calls (no locks). Cross-
    // session first, within-session second; the order matters only
    // when both fire in the same call (boundary AND threshold), and
    // in that case the cross summary covers the OLD session while
    // the within summary covers the NEW session, so they're
    // independent. Sequencing is for readability.
    let mut summary_contents: Vec<String> = Vec::with_capacity(pending.len());
    for batch in &pending {
        let content = run_summarization_call(provider, &batch.turns).await?;
        summary_contents.push(content);
    }

    // Phase 3 — write summary rows + read context for the operator-
    // facing call.
    let in_window: Vec<DecryptedTurn>;
    let summaries: Vec<DecryptedSummary>;
    {
        let domain_key = derive_conversation_key(vault_mutex)?;
        let mut conn = lock_db(db_mutex)?;
        let tx = conn.transaction().map_err(db::DbError::from)?;

        for (batch, content) in pending.iter().zip(summary_contents.iter()) {
            let generated_at = match &batch.generated_at_override {
                Some(s) => s.clone(),
                None => db::current_iso_timestamp(&tx)?,
            };
            db::put_summary(
                &tx,
                &domain_key,
                &batch.session_id,
                batch.kind,
                batch.range,
                content,
                &generated_at,
            )?;
        }

        in_window = db::list_turns_for_inference(&tx, &domain_key, &active_session_id)?;
        summaries = db::list_summaries_for_inference(&tx, &domain_key, &active_session_id)?;
        tx.commit().map_err(db::DbError::from)?;
    }

    // Phase 4 — operator-facing inference call (no locks). Synthetic
    // assistant message carrying the prepended summaries goes first,
    // then in-window verbatim turns in role order. The system prompt
    // is the §4 (a3) character + output discipline composition,
    // unchanged.
    let mut messages: Vec<inference::Message> = Vec::with_capacity(in_window.len() + 1);
    if !summaries.is_empty() {
        messages.push(inference::Message {
            role: inference::Role::Assistant,
            content: format_summaries_as_synthetic_assistant(&summaries),
        });
    }
    for t in in_window {
        messages.push(inference::Message {
            role: turn_role_to_inference(t.role),
            content: t.content,
        });
    }
    let request = inference::InferenceRequest {
        system_prompt: prompt::assemble_system_prompt().to_string(),
        messages,
    };
    let response = provider.infer(request).await?;

    // Phase 5 — write the assistant turn.
    let domain_key = derive_conversation_key(vault_mutex)?;
    let (assistant_turn_index, assistant_created_at) = {
        let mut conn = lock_db(db_mutex)?;
        let tx = conn.transaction().map_err(db::DbError::from)?;
        let turn_index = db::next_turn_index(&tx, &active_session_id)?;
        let created_at = db::current_iso_timestamp(&tx)?;
        db::put_turn(
            &tx,
            &domain_key,
            &active_session_id,
            turn_index,
            TurnRole::Assistant,
            &response.content,
            &created_at,
        )?;
        db::increment_session_turn_count(&tx, &active_session_id)?;
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
        session_id: active_session_id,
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

    // Wire-shape KAT for InferResponse. The React side depends on
    // this shape to splice the disk-authoritative turn indices and
    // timestamps into its scrollback after each round-trip. Drift in
    // any field name (assistant_content, turn_indices, created_at,
    // session_id) or in the nested struct shapes (user/assistant
    // pairs) breaks the channel surface's optimistic-replace path.
    //
    // §4 (c) extends the §4 (b) shape with `session_id` so the React
    // side stays in sync when the inactivity-gap boundary fires
    // inside `infer` and rolls the session.
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
            session_id: "s-current".into(),
        };
        assert_eq!(
            to_value(response).unwrap(),
            json!({
                "assistant_content": "yes.",
                "turn_indices": { "user": 4, "assistant": 5 },
                "created_at": {
                    "user": "2026-05-01T12:00:01.000Z",
                    "assistant": "2026-05-01T12:00:02.000Z"
                },
                "session_id": "s-current"
            })
        );
    }

    // §4 (c) — integration tests for the summarization triggers in
    // `infer_impl`. The Tauri command shells out to `infer_impl` so
    // these tests can drive the same flow without a Tauri runtime.
    // The stub provider is used so summarization round-trips are
    // deterministic and don't burn API tokens.

    use crate::crypto::Domain;
    use crate::inference::StubProvider;
    use crate::vault::setup_passphrase;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn open_in_memory_with_migrations() -> Connection {
        // The prod `db::open_and_migrate` resolves the operator's
        // ~/.coo path; tests use the test-only `run_migrations` helper
        // exposed by `db::test_support` instead.
        let mut conn = Connection::open_in_memory().expect("in-memory open");
        crate::db::test_support::run_migrations(&mut conn);
        conn
    }

    fn setup_vault_in_temp() -> (TempDir, UnlockedVault) {
        let dir = TempDir::new().unwrap();
        let vault = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        (dir, vault)
    }

    fn seed_turns(
        conn: &Connection,
        vault: &UnlockedVault,
        session_id: &str,
        count: usize,
    ) {
        let key = vault.domain_key(Domain::Conversation);
        for i in 0..count as i64 {
            db::put_turn(
                conn,
                &key,
                session_id,
                i,
                if i % 2 == 0 {
                    TurnRole::User
                } else {
                    TurnRole::Assistant
                },
                &format!("turn {i}"),
                "2026-05-01T12:00:00.000Z",
            )
            .unwrap();
            db::increment_session_turn_count(conn, session_id).unwrap();
        }
    }

    #[tokio::test]
    async fn infer_command_triggers_summarization_when_threshold_exceeded() {
        // Seed a session with INFERENCE_WINDOW_TURNS + SUMMARIZATION_BATCH_SIZE
        // turns; the next infer call adds one more (operator turn) and
        // pushes unsummarized count to threshold + 1, which trips the
        // within-session summarizer for the oldest SUMMARIZATION_BATCH_SIZE.
        let (_dir, vault) = setup_vault_in_temp();
        let conn = open_in_memory_with_migrations();
        let session_id = uuid::Uuid::new_v4().to_string();
        db::create_session(&conn, &session_id, "2026-05-01T12:00:00.000Z").unwrap();
        let seed_count = db::INFERENCE_WINDOW_TURNS + db::SUMMARIZATION_BATCH_SIZE;
        seed_turns(&conn, &vault, &session_id, seed_count);

        let db_mutex = Mutex::new(conn);
        let vault_mutex = Mutex::new(Some(vault));
        let provider = StubProvider::new();

        let response = infer_impl(
            &db_mutex,
            &vault_mutex,
            &provider,
            session_id.clone(),
            "next operator turn".into(),
        )
        .await
        .unwrap();

        // Same session — the within-session threshold should not
        // trigger a session boundary roll.
        assert_eq!(response.session_id, session_id);

        let conn = db_mutex.into_inner().unwrap();
        let summary_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM conversation_summary WHERE session_id = ?1",
                rusqlite::params![&session_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            summary_count, 1,
            "exactly one within-session summary must have been written"
        );

        let kind: String = conn
            .query_row(
                "SELECT summary_kind FROM conversation_summary WHERE session_id = ?1",
                rusqlite::params![&session_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "within_session");

        let sumthrough: i64 = conn
            .query_row(
                "SELECT summarized_through_turn_index FROM conversation_session WHERE id = ?1",
                rusqlite::params![&session_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            sumthrough,
            (db::SUMMARIZATION_BATCH_SIZE as i64) - 1,
            "summarized_through_turn_index must advance to cover the oldest batch"
        );
    }

    #[tokio::test]
    async fn infer_command_does_not_trigger_summarization_below_threshold() {
        // Seed a session well below threshold; infer should add one
        // operator turn + one assistant turn and write no summaries.
        let (_dir, vault) = setup_vault_in_temp();
        let conn = open_in_memory_with_migrations();
        let session_id = uuid::Uuid::new_v4().to_string();
        db::create_session(&conn, &session_id, "2026-05-01T12:00:00.000Z").unwrap();
        seed_turns(&conn, &vault, &session_id, 5);

        let db_mutex = Mutex::new(conn);
        let vault_mutex = Mutex::new(Some(vault));
        let provider = StubProvider::new();

        let response = infer_impl(
            &db_mutex,
            &vault_mutex,
            &provider,
            session_id.clone(),
            "another turn".into(),
        )
        .await
        .unwrap();

        assert_eq!(response.session_id, session_id);

        let conn = db_mutex.into_inner().unwrap();
        let summary_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversation_summary", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            summary_count, 0,
            "no summary should be written below threshold"
        );

        let sumthrough: i64 = conn
            .query_row(
                "SELECT summarized_through_turn_index FROM conversation_session WHERE id = ?1",
                rusqlite::params![&session_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sumthrough, -1, "marker must remain at default");
    }
}
