use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS subject (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    primary_name TEXT NOT NULL,
    date_of_birth TEXT,
    date_of_birth_year INTEGER,
    country TEXT,
    source TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS subject_alias (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject_id TEXT NOT NULL REFERENCES subject(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    alias_type TEXT NOT NULL,
    UNIQUE(subject_id, name, alias_type)
);

CREATE TABLE IF NOT EXISTS dataset_version (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
    record_count INTEGER NOT NULL DEFAULT 0,
    file_hash TEXT
);

CREATE INDEX IF NOT EXISTS idx_subject_source ON subject(source);
CREATE INDEX IF NOT EXISTS idx_subject_name ON subject(primary_name);
CREATE INDEX IF NOT EXISTS idx_alias_subject ON subject_alias(subject_id);
CREATE INDEX IF NOT EXISTS idx_alias_name ON subject_alias(name);
"#;

pub fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

pub fn record_dataset_version(conn: &Connection, source: &str, record_count: i64, file_hash: Option<&str>) -> Result<i64> {
    conn.execute(
        "INSERT INTO dataset_version (source, record_count, file_hash) VALUES (?1, ?2, ?3)",
        rusqlite::params![source, record_count, file_hash],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn schema_creates_tables() {
        let path = PathBuf::from(":memory:");
        let conn = open_db(&path).unwrap();
        init_schema(&conn).unwrap();
        
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM subject", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}



