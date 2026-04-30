use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};
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
}

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
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{decrypt, encrypt, Domain};
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
}
