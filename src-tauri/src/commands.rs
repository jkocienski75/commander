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
use crate::inference::InferenceProvider;
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
    #[allow(dead_code)] // §4 consumer
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
