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
    Migrations::new(vec![M::up(
        "CREATE TABLE _meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        ) STRICT;
        INSERT INTO _meta (key, value) VALUES ('initialized_at', datetime('now'));",
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_pass_validation() {
        // rusqlite_migration's validator runs each migration against a fresh
        // in-memory database. Catches typos and broken SQL at test time.
        migrations().validate().expect("migrations must validate");
    }
}
