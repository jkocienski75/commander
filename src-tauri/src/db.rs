use crate::crypto::{decrypt, encrypt, CryptoError, DomainKey};
use rusqlite::{params, Connection};
use rusqlite_migration::{M, Migrations};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("could not resolve home directory")]
    NoHomeDir,
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
}

// Schema version for §3 (b) tables. Every encrypted-row write commits to
// this value explicitly per RAPPORT-STATE-MODEL.md §7.2. A future
// non-additive migration introduces a new constant; the old constant
// stays alive for the lazy-migration read path.
const SCHEMA_VERSION_V1: i64 = 1;

// §4 (b) — maximum recent turns sent to inference per call. Older turns
// remain on disk and are loaded by `list_turns_for_ui` for display, but
// `list_turns_for_inference` caps at this value.
//
// This is the in-window tier of RAPPORT-STATE-MODEL.md §4.1's three-tier
// retention model. Tiers 2 (within-session summaries) and 3 (cross-
// session summaries) ship in §4 (c) and replace dropped turns with in-
// character summaries. Until §4 (c) lands, turns past the window are
// simply not sent to inference — they remain on disk and visible in
// the UI.
pub const INFERENCE_WINDOW_TURNS: usize = 100;

// SQLite strftime format used for plaintext `created_at` / `started_at`
// columns. UTC, ISO 8601 with millisecond precision and explicit `Z`
// suffix. JS `new Date(...)` parses this round-trip.
const ISO_TIMESTAMP_FMT: &str = "%Y-%m-%dT%H:%M:%fZ";

// Per coo/CLAUDE.md "State stays in ~/.coo/" and RAPPORT-STATE-MODEL.md §6.
pub fn db_path() -> Result<PathBuf, DbError> {
    let home = dirs::home_dir().ok_or(DbError::NoHomeDir)?;
    Ok(home.join(".coo").join("coo.db"))
}

pub fn open_and_migrate() -> Result<Connection, DbError> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(&path)?;
    migrations().to_latest(&mut conn)?;
    Ok(conn)
}

// Append-only. Per RAPPORT-STATE-MODEL.md §7: strict-additive default;
// non-additive changes use versioned envelopes per §7.2. Existing migrations
// are never edited after they ship.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(
            "CREATE TABLE _meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            ) STRICT;
            INSERT INTO _meta (key, value) VALUES ('initialized_at', datetime('now'));",
        ),
        // §3 (b) — schema for the onboarding wizard's writes.
        //
        // app_config is plaintext per RAPPORT-STATE-MODEL.md §6.4's narrow
        // permitted-plaintext list (theme, basic onboarding state, schema
        // metadata). It is NOT a place for state-domain content — operator
        // knowledge, rapport state, friendship floor, conversation — those
        // belong in their own encrypted-column tables under domain keys
        // derived from UnlockedVault per §6.3.
        //
        // operator_profile carries the wizard's "basic operator profile" as
        // a single AEAD bundle (the §2 (c) v1 envelope: 6-byte header +
        // 24-byte nonce + ct+tag) under Domain::OperatorKnowledge. The
        // singleton CHECK (id = 1) enforces single-operator structurally —
        // ADR-0011's single-operator commitment becomes a SQL-layer rule.
        //
        // calibration_setting is the placeholder shape for the ten dials
        // enumerated in EXILE.md §3 (Texture / Posture / Currency /
        // Foundation groups). Internal representation of dial values is
        // deliberately *not* typed here: EXILE.md §3 names dial endpoints
        // (cool ↔ open) but does not commit to enum-vs-float-vs-step
        // quantization, and §4's Familiar preset uses hand-tuned per-dial
        // labels rather than a uniform shape. The typed schema lands at
        // Phase 1 §6 (Calibration surface). Until then the dial_key is a
        // plaintext string (the dial names are public doctrine — EXILE
        // §3.1–§3.4); the chosen value is the encrypted payload.
        //
        // schema_version is required-explicit (no DEFAULT). §7.2's
        // versioned-envelope discipline turns on every row committing to
        // its own version; defaulting hides that commitment.
        //
        // updated_at is plaintext for ordering / audit; it leaks "the
        // operator changed something at time T" but no content.
        //
        // No semantic AAD on the bundles in §3 (b) — the §2 (c) AAD is
        // just the 6-byte header. Adding row-identity AAD is a v2 bundle
        // bump tracked in CLAUDE.md "Documentary debt to retire".
        M::up(
            "CREATE TABLE app_config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            ) STRICT;
            CREATE TABLE operator_profile (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                ciphertext BLOB NOT NULL,
                schema_version INTEGER NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            ) STRICT;
            CREATE TABLE calibration_setting (
                dial_key TEXT PRIMARY KEY,
                ciphertext BLOB NOT NULL,
                schema_version INTEGER NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            ) STRICT;",
        ),
        // §4 (b) — conversation persistence.
        //
        // Two tables per RAPPORT-STATE-MODEL.md §2.4. `conversation_session`
        // groups turns; `conversation_turn` is the encrypted turn body. The
        // §2.4-spec'd `conversation_summary` table is deferred to §4 (c) —
        // it isn't exercised until summarization writes start, and migrating
        // it now would pre-commit a shape that hasn't been tested against
        // real conversation flow.
        //
        // Both `id` columns are TEXT to hold UUIDs (string-rendered) per
        // the precedent in `RAPPORT-STATE-MODEL.md` §2.3 (operator-knowledge
        // entries also UUID-keyed).
        //
        // `role` is a plaintext CHECK-constrained TEXT column rather than
        // encrypted. The role distinction (user vs. assistant) is structural
        // — it shapes the role of each turn in the inference request — and
        // is not sensitive on its own. Encrypting it would mean decrypting
        // every row on read just to know which speaker; cost is real,
        // protection is nil.
        //
        // `ciphertext` carries the §2 (c) v1 AEAD bundle under
        // `Domain::Conversation` — the first state-domain table in the
        // schema to actually use the conversation domain key. Same
        // encrypted-column convention as §3 (b) (operator_profile,
        // calibration_setting under OperatorKnowledge).
        //
        // `created_at` is plaintext for ordering and for the date-divider
        // rendering on the React side. Leaks "operator was active at time
        // T" but no content. Same trade as §3 (b)'s plaintext timestamps.
        //
        // `schema_version` is required-explicit (no DEFAULT) per §7.2.
        //
        // `UNIQUE (session_id, turn_index)` makes the index ordering
        // structural and catches double-writes if a future bug or retry
        // path tries to insert the same turn twice. The supporting index
        // makes the load-path SELECT cheap regardless of total turn
        // volume.
        M::up(
            "CREATE TABLE conversation_session (
                id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                turn_count INTEGER NOT NULL DEFAULT 0,
                schema_version INTEGER NOT NULL
            ) STRICT;
            CREATE TABLE conversation_turn (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                turn_index INTEGER NOT NULL,
                role TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
                ciphertext BLOB NOT NULL,
                created_at TEXT NOT NULL,
                schema_version INTEGER NOT NULL,
                FOREIGN KEY (session_id) REFERENCES conversation_session(id),
                UNIQUE (session_id, turn_index)
            ) STRICT;
            CREATE INDEX idx_conversation_turn_session_index
                ON conversation_turn(session_id, turn_index);",
        ),
    ])
}

// Pure-Rust write helpers consumed by the Tauri commands in `commands`.
// Kept separate from the #[tauri::command] wrappers so they can be unit-
// tested without a Tauri runtime. INSERT-or-REPLACE semantics on the
// encrypted tables: the wizard may re-submit a step (the operator
// changes their callsign before completing onboarding); the typed §6
// surfaces will overwrite the same singleton/keyed rows on every change.
//
// All three helpers update updated_at via SQLite's CURRENT_TIMESTAMP
// default by leaving the column out of the UPSERT payload — INSERT
// applies the default; ON CONFLICT DO UPDATE explicitly rewrites it.

pub fn put_app_config(conn: &Connection, key: &str, value: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET
             value = excluded.value,
             updated_at = CURRENT_TIMESTAMP",
        params![key, value],
    )?;
    Ok(())
}

pub fn put_operator_profile(
    conn: &Connection,
    key: &DomainKey,
    plaintext: &[u8],
) -> Result<(), DbError> {
    let bundle = encrypt(key, plaintext)?;
    conn.execute(
        "INSERT INTO operator_profile (id, ciphertext, schema_version) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET
             ciphertext = excluded.ciphertext,
             schema_version = excluded.schema_version,
             updated_at = CURRENT_TIMESTAMP",
        params![bundle, SCHEMA_VERSION_V1],
    )?;
    Ok(())
}

pub fn put_calibration_setting(
    conn: &Connection,
    key: &DomainKey,
    dial_key: &str,
    plaintext: &[u8],
) -> Result<(), DbError> {
    let bundle = encrypt(key, plaintext)?;
    conn.execute(
        "INSERT INTO calibration_setting (dial_key, ciphertext, schema_version) VALUES (?1, ?2, ?3)
         ON CONFLICT(dial_key) DO UPDATE SET
             ciphertext = excluded.ciphertext,
             schema_version = excluded.schema_version,
             updated_at = CURRENT_TIMESTAMP",
        params![dial_key, bundle, SCHEMA_VERSION_V1],
    )?;
    Ok(())
}

// §4 (b) — conversation persistence helpers.
//
// `TurnRole` mirrors `inference::Role` but is intentionally a separate
// type so the db layer doesn't depend on the inference module. The
// dependency direction is one-way: inference → db is fine, db →
// inference would be a layering inversion. Translation between the
// two enums happens at the command layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnRole {
    User,
    Assistant,
}

impl TurnRole {
    fn as_str(self) -> &'static str {
        match self {
            TurnRole::User => "user",
            TurnRole::Assistant => "assistant",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(TurnRole::User),
            "assistant" => Some(TurnRole::Assistant),
            _ => None,
        }
    }
}

// Decrypted turn payload returned by `list_turns_for_*`. The `content`
// field is the post-decryption plaintext from the AEAD bundle; the
// other fields are the plaintext columns from `conversation_turn`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecryptedTurn {
    pub turn_index: i64,
    pub role: TurnRole,
    pub content: String,
    pub created_at: String,
}

// Insert a session row. Caller chooses `id` (typically a fresh UUID)
// and `started_at` (typically the current ISO timestamp). `ended_at`
// is left NULL; `turn_count` defaults to 0.
pub fn create_session(conn: &Connection, id: &str, started_at: &str) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO conversation_session (id, started_at, schema_version)
         VALUES (?1, ?2, ?3)",
        params![id, started_at, SCHEMA_VERSION_V1],
    )?;
    Ok(())
}

// Insert a turn under an existing session. `turn_index` is supplied by
// the caller (typically `MAX(turn_index)+1` within the session); the
// UNIQUE (session_id, turn_index) constraint catches double-writes.
//
// The `id` is generated here as a fresh UUID v4; turns don't carry
// caller-meaningful identity beyond (session_id, turn_index), and the
// uuid generation is cheap.
//
// The `&Connection` parameter accepts both a raw connection and a
// `&Transaction` (via `Deref<Target = Connection>`), so the caller can
// wrap turn-insert + session turn_count UPDATE in one transaction
// without changing the helper's signature.
pub fn put_turn(
    conn: &Connection,
    domain_key: &DomainKey,
    session_id: &str,
    turn_index: i64,
    role: TurnRole,
    content: &str,
    created_at: &str,
) -> Result<(), DbError> {
    let bundle = encrypt(domain_key, content.as_bytes())?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO conversation_turn
            (id, session_id, turn_index, role, ciphertext, created_at, schema_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            session_id,
            turn_index,
            role.as_str(),
            bundle,
            created_at,
            SCHEMA_VERSION_V1,
        ],
    )?;
    Ok(())
}

// Returns *all* turns for a session, ascending by turn_index. Used by
// the React load-on-mount path; the operator can scroll back to
// anything they have ever said.
pub fn list_turns_for_ui(
    conn: &Connection,
    domain_key: &DomainKey,
    session_id: &str,
) -> Result<Vec<DecryptedTurn>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT turn_index, role, ciphertext, created_at
         FROM conversation_turn
         WHERE session_id = ?1
         ORDER BY turn_index ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (turn_index, role_s, bundle, created_at) = row?;
        let role = TurnRole::from_str(&role_s).ok_or_else(|| {
            DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                format!("unknown role {role_s:?} in conversation_turn").into(),
            ))
        })?;
        let content_bytes = decrypt(domain_key, &bundle)?;
        let content = String::from_utf8(content_bytes).map_err(|e| {
            DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Blob,
                format!("turn ciphertext decoded to invalid utf-8: {e}").into(),
            ))
        })?;
        out.push(DecryptedTurn {
            turn_index,
            role,
            content,
            created_at,
        });
    }
    Ok(out)
}

// Returns the *most recent* `INFERENCE_WINDOW_TURNS` turns for a
// session, ascending by turn_index. Used by the `infer` command —
// older turns are not sent to inference but remain on disk and are
// returned by `list_turns_for_ui`.
//
// Two functions rather than parameterized: callers should not be
// making the choice at call sites. The UI load path always wants
// everything; the inference path always wants the window.
pub fn list_turns_for_inference(
    conn: &Connection,
    domain_key: &DomainKey,
    session_id: &str,
) -> Result<Vec<DecryptedTurn>, DbError> {
    // SELECT DESC + LIMIT, then reverse in Rust — the standard SQLite
    // idiom for "most recent N rows" without a window function.
    let mut stmt = conn.prepare(
        "SELECT turn_index, role, ciphertext, created_at
         FROM conversation_turn
         WHERE session_id = ?1
         ORDER BY turn_index DESC
         LIMIT ?2",
    )?;
    let limit = INFERENCE_WINDOW_TURNS as i64;
    let rows = stmt.query_map(params![session_id, limit], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (turn_index, role_s, bundle, created_at) = row?;
        let role = TurnRole::from_str(&role_s).ok_or_else(|| {
            DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                format!("unknown role {role_s:?} in conversation_turn").into(),
            ))
        })?;
        let content_bytes = decrypt(domain_key, &bundle)?;
        let content = String::from_utf8(content_bytes).map_err(|e| {
            DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Blob,
                format!("turn ciphertext decoded to invalid utf-8: {e}").into(),
            ))
        })?;
        out.push(DecryptedTurn {
            turn_index,
            role,
            content,
            created_at,
        });
    }
    out.reverse();
    Ok(out)
}

// Helper for the command layer: produce a fresh ISO 8601 timestamp
// using SQLite's clock so all conversation timestamps share one
// canonical source. Avoids an extra `chrono` / `time` dependency.
pub fn current_iso_timestamp(conn: &Connection) -> Result<String, DbError> {
    let ts: String = conn.query_row(
        "SELECT strftime(?1, 'now')",
        params![ISO_TIMESTAMP_FMT],
        |r| r.get(0),
    )?;
    Ok(ts)
}

// Returns the most-recent session's id, or None if no sessions exist.
// Sorting on `started_at` not `id` because IDs are UUIDs (no ordering
// implied by the uuid bytes themselves).
pub fn latest_session_id(conn: &Connection) -> Result<Option<String>, DbError> {
    match conn.query_row(
        "SELECT id FROM conversation_session ORDER BY started_at DESC LIMIT 1",
        [],
        |r| r.get::<_, String>(0),
    ) {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(DbError::from(e)),
    }
}

// Returns the next turn_index for a session (i.e. MAX(turn_index)+1
// within the session, or 0 if no turns exist).
pub fn next_turn_index(conn: &Connection, session_id: &str) -> Result<i64, DbError> {
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(turn_index), -1) + 1
         FROM conversation_turn
         WHERE session_id = ?1",
        params![session_id],
        |r| r.get(0),
    )?;
    Ok(next)
}

// Increments `turn_count` by 1 on the given session. Caller is
// expected to wrap this with the corresponding `put_turn` in a
// single SQLite transaction so the count cannot drift from the
// actual turn rows.
pub fn increment_session_turn_count(
    conn: &Connection,
    session_id: &str,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE conversation_session
         SET turn_count = turn_count + 1
         WHERE id = ?1",
        params![session_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{decrypt, Domain};
    use crate::vault::setup_passphrase;
    use rusqlite::params;
    use tempfile::TempDir;

    #[test]
    fn migrations_pass_validation() {
        // rusqlite_migration's validator runs each migration against a fresh
        // in-memory database. Catches typos and broken SQL at test time.
        migrations().validate().expect("migrations must validate");
    }

    fn open_in_memory() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory open");
        migrations().to_latest(&mut conn).expect("migrate to latest");
        conn
    }

    #[test]
    fn app_config_plaintext_roundtrip() {
        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('theme', 'secret_agent')",
            [],
        )
        .unwrap();
        let value: String = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = 'theme'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "secret_agent");
    }

    #[test]
    fn operator_profile_singleton_enforced() {
        // CHECK (id = 1) makes single-operator a SQL-layer invariant rather
        // than relying on application code to never write a second row.
        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO operator_profile (id, ciphertext, schema_version) VALUES (1, X'00', 1)",
            [],
        )
        .unwrap();
        let second = conn.execute(
            "INSERT INTO operator_profile (id, ciphertext, schema_version) VALUES (2, X'00', 1)",
            [],
        );
        assert!(second.is_err(), "CHECK (id = 1) must reject id != 1");
    }

    #[test]
    fn operator_profile_encrypted_roundtrip() {
        // End-to-end proof: a vault unlocked from the §3 (a) substrate
        // composes with the §3 (b) schema. The encrypted-column pattern
        // works against the operator-knowledge domain key.
        let dir = TempDir::new().unwrap();
        let vault = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        let key = vault.domain_key(Domain::OperatorKnowledge);
        let plaintext = b"{\"callsign\":\"Cardinal-7\"}";
        let bundle = encrypt(&key, plaintext).unwrap();

        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO operator_profile (id, ciphertext, schema_version) VALUES (1, ?1, 1)",
            params![bundle],
        )
        .unwrap();

        let stored: Vec<u8> = conn
            .query_row(
                "SELECT ciphertext FROM operator_profile WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, bundle, "BLOB column must round-trip exact bytes");

        let decrypted = decrypt(&key, &stored).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn put_app_config_upserts() {
        let conn = open_in_memory();
        put_app_config(&conn, "theme", "secret_agent").unwrap();
        put_app_config(&conn, "theme", "secret_agent_v2").unwrap();
        let value: String = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = 'theme'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "secret_agent_v2");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM app_config", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "upsert must not create a duplicate row");
    }

    #[test]
    fn put_operator_profile_upserts_singleton() {
        // Second call to put_operator_profile must overwrite the singleton
        // row, not fail on the CHECK (id = 1) constraint.
        let dir = TempDir::new().unwrap();
        let vault = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        let key = vault.domain_key(Domain::OperatorKnowledge);
        let conn = open_in_memory();

        put_operator_profile(&conn, &key, b"{\"callsign\":\"Cardinal-7\"}").unwrap();
        put_operator_profile(&conn, &key, b"{\"callsign\":\"Cardinal\"}").unwrap();

        let stored: Vec<u8> = conn
            .query_row(
                "SELECT ciphertext FROM operator_profile WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let decrypted = decrypt(&key, &stored).unwrap();
        assert_eq!(decrypted.as_slice(), b"{\"callsign\":\"Cardinal\"}");
    }

    #[test]
    fn put_calibration_setting_upserts_per_key() {
        let dir = TempDir::new().unwrap();
        let vault = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        let key = vault.domain_key(Domain::OperatorKnowledge);
        let conn = open_in_memory();

        put_calibration_setting(&conn, &key, "warmth", b"present").unwrap();
        put_calibration_setting(&conn, &key, "warmth", b"open").unwrap();
        put_calibration_setting(&conn, &key, "discipline", b"exacting").unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM calibration_setting", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "two distinct keys, no duplicates from upsert");

        let warmth: Vec<u8> = conn
            .query_row(
                "SELECT ciphertext FROM calibration_setting WHERE dial_key = 'warmth'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(decrypt(&key, &warmth).unwrap().as_slice(), b"open");
    }

    #[test]
    fn calibration_setting_encrypted_roundtrip() {
        // Same encrypted-column pattern, exercised on multiple rows. The
        // dial_key strings are plaintext (public doctrine per EXILE §3); the
        // chosen settings are the secret payload.
        let dir = TempDir::new().unwrap();
        let vault = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        let key = vault.domain_key(Domain::OperatorKnowledge);

        let entries: [(&str, &[u8]); 3] = [
            ("warmth", b"present"),
            ("discipline", b"exacting"),
            ("flirtation", b"low"),
        ];

        let conn = open_in_memory();
        for (dial, value) in entries.iter() {
            let bundle = encrypt(&key, value).unwrap();
            conn.execute(
                "INSERT INTO calibration_setting (dial_key, ciphertext, schema_version) VALUES (?1, ?2, 1)",
                params![dial, bundle],
            )
            .unwrap();
        }

        for (dial, value) in entries.iter() {
            let stored: Vec<u8> = conn
                .query_row(
                    "SELECT ciphertext FROM calibration_setting WHERE dial_key = ?1",
                    params![dial],
                    |r| r.get(0),
                )
                .unwrap();
            let decrypted = decrypt(&key, &stored).unwrap();
            assert_eq!(decrypted.as_slice(), *value);
        }
    }

    // §4 (b) — schema and helper tests for conversation persistence.

    fn conversation_key() -> (TempDir, DomainKey) {
        let dir = TempDir::new().unwrap();
        let vault = setup_passphrase(dir.path(), b"operator-passphrase").unwrap();
        let key = vault.domain_key(Domain::Conversation);
        (dir, key)
    }

    #[test]
    fn conversation_session_roundtrip() {
        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO conversation_session (id, started_at, schema_version)
             VALUES ('s-1', '2026-05-01T12:00:00.000Z', 1)",
            [],
        )
        .unwrap();
        let (id, started_at, ended_at, turn_count): (String, String, Option<String>, i64) = conn
            .query_row(
                "SELECT id, started_at, ended_at, turn_count
                 FROM conversation_session WHERE id = 's-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(id, "s-1");
        assert_eq!(started_at, "2026-05-01T12:00:00.000Z");
        assert_eq!(ended_at, None);
        assert_eq!(turn_count, 0, "default turn_count is 0");
    }

    #[test]
    fn conversation_turn_unique_session_index_enforced() {
        // The UNIQUE (session_id, turn_index) constraint catches double-
        // writes if a future bug or retry path tries to insert the same
        // turn twice.
        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO conversation_session (id, started_at, schema_version)
             VALUES ('s-1', '2026-05-01T12:00:00.000Z', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_turn
                (id, session_id, turn_index, role, ciphertext, created_at, schema_version)
             VALUES ('t-1', 's-1', 0, 'user', X'00', '2026-05-01T12:00:01.000Z', 1)",
            [],
        )
        .unwrap();
        let dup = conn.execute(
            "INSERT INTO conversation_turn
                (id, session_id, turn_index, role, ciphertext, created_at, schema_version)
             VALUES ('t-2', 's-1', 0, 'user', X'00', '2026-05-01T12:00:02.000Z', 1)",
            [],
        );
        assert!(dup.is_err(), "duplicate (session_id, turn_index) must reject");
    }

    #[test]
    fn conversation_turn_encrypted_roundtrip() {
        // End-to-end: vault → Domain::Conversation key → encrypt →
        // INSERT → SELECT → decrypt. First exercise of the conversation
        // domain key against a state-domain table.
        let (_dir, key) = conversation_key();
        let plaintext = b"the operator just spoke";
        let bundle = encrypt(&key, plaintext).unwrap();

        let conn = open_in_memory();
        conn.execute(
            "INSERT INTO conversation_session (id, started_at, schema_version)
             VALUES ('s-1', '2026-05-01T12:00:00.000Z', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_turn
                (id, session_id, turn_index, role, ciphertext, created_at, schema_version)
             VALUES ('t-1', 's-1', 0, 'user', ?1, '2026-05-01T12:00:01.000Z', 1)",
            params![bundle],
        )
        .unwrap();

        let stored: Vec<u8> = conn
            .query_row(
                "SELECT ciphertext FROM conversation_turn WHERE id = 't-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, bundle, "BLOB column must round-trip exact bytes");

        let decrypted = decrypt(&key, &stored).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn create_session_and_put_turn() {
        let (_dir, key) = conversation_key();
        let conn = open_in_memory();

        create_session(&conn, "s-1", "2026-05-01T12:00:00.000Z").unwrap();
        put_turn(
            &conn,
            &key,
            "s-1",
            0,
            TurnRole::User,
            "hi there",
            "2026-05-01T12:00:01.000Z",
        )
        .unwrap();

        let turns = list_turns_for_ui(&conn, &key, "s-1").unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].turn_index, 0);
        assert_eq!(turns[0].role, TurnRole::User);
        assert_eq!(turns[0].content, "hi there");
        assert_eq!(turns[0].created_at, "2026-05-01T12:00:01.000Z");
    }

    #[test]
    fn put_turn_rejects_duplicate_session_index() {
        // Same UNIQUE constraint as the schema test, exercised through
        // the public helper.
        let (_dir, key) = conversation_key();
        let conn = open_in_memory();
        create_session(&conn, "s-1", "2026-05-01T12:00:00.000Z").unwrap();
        put_turn(
            &conn,
            &key,
            "s-1",
            0,
            TurnRole::User,
            "first",
            "2026-05-01T12:00:01.000Z",
        )
        .unwrap();
        let dup = put_turn(
            &conn,
            &key,
            "s-1",
            0,
            TurnRole::Assistant,
            "second",
            "2026-05-01T12:00:02.000Z",
        );
        assert!(dup.is_err(), "second put_turn at index 0 must fail");
    }

    #[test]
    fn list_turns_for_ui_returns_all_in_order() {
        // Insert turns out of order and assert SELECT returns them
        // ascending — the ORDER BY in the SQL is what we're locking.
        let (_dir, key) = conversation_key();
        let conn = open_in_memory();
        create_session(&conn, "s-1", "2026-05-01T12:00:00.000Z").unwrap();
        for &i in &[2_i64, 0, 3, 1] {
            put_turn(
                &conn,
                &key,
                "s-1",
                i,
                TurnRole::User,
                &format!("turn {i}"),
                "2026-05-01T12:00:01.000Z",
            )
            .unwrap();
        }
        let turns = list_turns_for_ui(&conn, &key, "s-1").unwrap();
        assert_eq!(turns.len(), 4);
        assert_eq!(turns[0].turn_index, 0);
        assert_eq!(turns[1].turn_index, 1);
        assert_eq!(turns[2].turn_index, 2);
        assert_eq!(turns[3].turn_index, 3);
    }

    #[test]
    fn list_turns_for_inference_caps_at_window() {
        // Insert N+50 turns; assert exactly INFERENCE_WINDOW_TURNS come
        // back, and that they are the *most recent* — the SQL is DESC +
        // LIMIT and we want it to stay that way.
        let (_dir, key) = conversation_key();
        let conn = open_in_memory();
        create_session(&conn, "s-1", "2026-05-01T12:00:00.000Z").unwrap();

        let total = INFERENCE_WINDOW_TURNS as i64 + 50;
        for i in 0..total {
            put_turn(
                &conn,
                &key,
                "s-1",
                i,
                TurnRole::User,
                &format!("turn {i}"),
                "2026-05-01T12:00:01.000Z",
            )
            .unwrap();
        }

        let turns = list_turns_for_inference(&conn, &key, "s-1").unwrap();
        assert_eq!(turns.len(), INFERENCE_WINDOW_TURNS);
        // Most-recent N: turn_index range is [50, total).
        let expected_first = total - INFERENCE_WINDOW_TURNS as i64;
        assert_eq!(turns[0].turn_index, expected_first);
        assert_eq!(turns[INFERENCE_WINDOW_TURNS - 1].turn_index, total - 1);
    }

    #[test]
    fn list_turns_for_inference_returns_ascending() {
        // Catches a regression where the reverse step gets dropped in
        // a future refactor — without the in-Rust `.reverse()`, the SQL
        // DESC + LIMIT would surface the most-recent N in reverse.
        let (_dir, key) = conversation_key();
        let conn = open_in_memory();
        create_session(&conn, "s-1", "2026-05-01T12:00:00.000Z").unwrap();
        for i in 0..5_i64 {
            put_turn(
                &conn,
                &key,
                "s-1",
                i,
                TurnRole::User,
                &format!("turn {i}"),
                "2026-05-01T12:00:01.000Z",
            )
            .unwrap();
        }
        let turns = list_turns_for_inference(&conn, &key, "s-1").unwrap();
        let indices: Vec<i64> = turns.iter().map(|t| t.turn_index).collect();
        assert_eq!(indices, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn next_turn_index_starts_at_zero_and_advances() {
        let (_dir, key) = conversation_key();
        let conn = open_in_memory();
        create_session(&conn, "s-1", "2026-05-01T12:00:00.000Z").unwrap();
        assert_eq!(next_turn_index(&conn, "s-1").unwrap(), 0);
        put_turn(
            &conn,
            &key,
            "s-1",
            0,
            TurnRole::User,
            "first",
            "2026-05-01T12:00:01.000Z",
        )
        .unwrap();
        assert_eq!(next_turn_index(&conn, "s-1").unwrap(), 1);
        put_turn(
            &conn,
            &key,
            "s-1",
            1,
            TurnRole::Assistant,
            "second",
            "2026-05-01T12:00:02.000Z",
        )
        .unwrap();
        assert_eq!(next_turn_index(&conn, "s-1").unwrap(), 2);
    }

    #[test]
    fn latest_session_id_returns_most_recent_by_started_at() {
        let conn = open_in_memory();
        assert!(latest_session_id(&conn).unwrap().is_none());
        create_session(&conn, "s-old", "2026-05-01T08:00:00.000Z").unwrap();
        create_session(&conn, "s-new", "2026-05-01T20:00:00.000Z").unwrap();
        create_session(&conn, "s-mid", "2026-05-01T12:00:00.000Z").unwrap();
        assert_eq!(
            latest_session_id(&conn).unwrap(),
            Some("s-new".to_string())
        );
    }
}
