use anyhow::Result;
use rusqlite::Connection;

/// Subject that needs to be re-screened when lists update
#[derive(Debug, Clone)]
pub struct MonitoredSubject {
    pub id: i64,
    pub tenant_id: String,
    pub reference_id: String,
    pub name: String,
    pub country: Option<String>,
    pub dob_year: Option<i32>,
    pub last_screened_at: String,
    pub last_result_hash: Option<String>,
    pub callback_url: Option<String>,
}

/// Result of a monitoring check
#[derive(Debug, Clone)]
pub struct MonitoringResult {
    pub subject_id: i64,
    pub reference_id: String,
    pub has_changes: bool,
    pub new_result_hash: String,
    pub hit_count: usize,
    pub highest_score: f32,
}

/// Initialize monitoring tables in the database
pub fn init_monitoring_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS monitored_subject (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            tenant_id TEXT NOT NULL,
            reference_id TEXT NOT NULL,
            name TEXT NOT NULL,
            country TEXT,
            dob_year INTEGER,
            last_screened_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_result_hash TEXT,
            callback_url TEXT,
            active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(tenant_id, reference_id)
        );

        CREATE INDEX IF NOT EXISTS idx_monitored_tenant 
            ON monitored_subject(tenant_id, active);

        CREATE TABLE IF NOT EXISTS monitoring_result (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            subject_id INTEGER NOT NULL REFERENCES monitored_subject(id),
            screened_at TEXT NOT NULL DEFAULT (datetime('now')),
            result_hash TEXT NOT NULL,
            hit_count INTEGER NOT NULL,
            highest_score REAL NOT NULL,
            has_changes INTEGER NOT NULL,
            notified INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_monitoring_result_subject 
            ON monitoring_result(subject_id, screened_at DESC);
        "
    )?;

    tracing::info!("monitoring schema initialized");
    Ok(())
}

/// Add a subject to monitoring
pub fn add_monitored_subject(
    conn: &Connection,
    tenant_id: &str,
    reference_id: &str,
    name: &str,
    country: Option<&str>,
    dob_year: Option<i32>,
    callback_url: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT OR REPLACE INTO monitored_subject 
         (tenant_id, reference_id, name, country, dob_year, callback_url, active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
        rusqlite::params![tenant_id, reference_id, name, country, dob_year, callback_url],
    )?;

    let id = conn.last_insert_rowid();
    tracing::debug!(id, reference_id, "added subject to monitoring");
    Ok(id)
}

/// Remove a subject from monitoring
pub fn remove_monitored_subject(
    conn: &Connection,
    tenant_id: &str,
    reference_id: &str,
) -> Result<bool> {
    let rows = conn.execute(
        "UPDATE monitored_subject SET active = 0 
         WHERE tenant_id = ?1 AND reference_id = ?2",
        rusqlite::params![tenant_id, reference_id],
    )?;
    Ok(rows > 0)
}

/// Get all active monitored subjects for a tenant
pub fn get_monitored_subjects(
    conn: &Connection,
    tenant_id: &str,
) -> Result<Vec<MonitoredSubject>> {
    let mut stmt = conn.prepare(
        "SELECT id, tenant_id, reference_id, name, country, dob_year, 
                last_screened_at, last_result_hash, callback_url
         FROM monitored_subject 
         WHERE tenant_id = ?1 AND active = 1"
    )?;

    let subjects = stmt.query_map([tenant_id], |row| {
        Ok(MonitoredSubject {
            id: row.get(0)?,
            tenant_id: row.get(1)?,
            reference_id: row.get(2)?,
            name: row.get(3)?,
            country: row.get(4)?,
            dob_year: row.get(5)?,
            last_screened_at: row.get(6)?,
            last_result_hash: row.get(7)?,
            callback_url: row.get(8)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(subjects)
}

/// Get all active subjects across all tenants (for batch re-screening)
pub fn get_all_active_subjects(conn: &Connection) -> Result<Vec<MonitoredSubject>> {
    let mut stmt = conn.prepare(
        "SELECT id, tenant_id, reference_id, name, country, dob_year, 
                last_screened_at, last_result_hash, callback_url
         FROM monitored_subject 
         WHERE active = 1
         ORDER BY last_screened_at ASC"
    )?;

    let subjects = stmt.query_map([], |row| {
        Ok(MonitoredSubject {
            id: row.get(0)?,
            tenant_id: row.get(1)?,
            reference_id: row.get(2)?,
            name: row.get(3)?,
            country: row.get(4)?,
            dob_year: row.get(5)?,
            last_screened_at: row.get(6)?,
            last_result_hash: row.get(7)?,
            callback_url: row.get(8)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(subjects)
}

/// Record a monitoring result
pub fn record_monitoring_result(
    conn: &Connection,
    subject_id: i64,
    result_hash: &str,
    hit_count: usize,
    highest_score: f32,
    has_changes: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO monitoring_result 
         (subject_id, result_hash, hit_count, highest_score, has_changes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![subject_id, result_hash, hit_count as i64, highest_score, has_changes as i32],
    )?;

    // Update last_screened_at and last_result_hash
    conn.execute(
        "UPDATE monitored_subject 
         SET last_screened_at = datetime('now'), last_result_hash = ?2
         WHERE id = ?1",
        rusqlite::params![subject_id, result_hash],
    )?;

    Ok(())
}

/// Get subjects with changes that haven't been notified
pub fn get_pending_notifications(conn: &Connection) -> Result<Vec<(MonitoredSubject, MonitoringResult, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.tenant_id, s.reference_id, s.name, s.country, s.dob_year,
                s.last_screened_at, s.last_result_hash, s.callback_url,
                r.id, r.result_hash, r.hit_count, r.highest_score, r.has_changes
         FROM monitored_subject s
         JOIN monitoring_result r ON s.id = r.subject_id
         WHERE r.has_changes = 1 AND r.notified = 0
         ORDER BY r.screened_at DESC"
    )?;

    let results = stmt.query_map([], |row| {
        Ok((
            MonitoredSubject {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                reference_id: row.get(2)?,
                name: row.get(3)?,
                country: row.get(4)?,
                dob_year: row.get(5)?,
                last_screened_at: row.get(6)?,
                last_result_hash: row.get(7)?,
                callback_url: row.get(8)?,
            },
            MonitoringResult {
                subject_id: row.get(0)?,
                reference_id: row.get(2)?,
                has_changes: row.get::<_, i32>(13)? == 1,
                new_result_hash: row.get(10)?,
                hit_count: row.get::<_, i64>(11)? as usize,
                highest_score: row.get(12)?,
            },
            row.get::<_, i64>(9)?, // result_id
        ))
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

/// Mark a notification as sent
pub fn mark_notified(conn: &Connection, result_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE monitoring_result SET notified = 1 WHERE id = ?1",
        [result_id],
    )?;
    Ok(())
}

/// Compute a hash of screening results for change detection
pub fn compute_result_hash(hits: &[(String, f32)]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    for (id, score) in hits {
        id.hash(&mut hasher);
        // Round score to 2 decimals for stability
        ((score * 100.0) as i32).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_db;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn monitoring_workflow() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let conn = open_db(&db_path).unwrap();
        crate::db::init_schema(&conn).unwrap();
        init_monitoring_schema(&conn).unwrap();

        // Add subject
        let id = add_monitored_subject(
            &conn, "tenant1", "ref1", "John Doe", Some("US"), Some(1980), None
        ).unwrap();
        assert!(id > 0);

        // Get subjects
        let subjects = get_monitored_subjects(&conn, "tenant1").unwrap();
        assert_eq!(subjects.len(), 1);
        assert_eq!(subjects[0].name, "John Doe");

        // Record result
        record_monitoring_result(&conn, id, "hash123", 2, 0.85, false).unwrap();

        // Remove subject
        let removed = remove_monitored_subject(&conn, "tenant1", "ref1").unwrap();
        assert!(removed);

        // Should not appear in active list
        let subjects = get_monitored_subjects(&conn, "tenant1").unwrap();
        assert_eq!(subjects.len(), 0);
    }

    #[test]
    fn result_hash_stability() {
        let hits1 = vec![("id1".to_string(), 0.95f32), ("id2".to_string(), 0.80f32)];
        let hits2 = vec![("id1".to_string(), 0.95f32), ("id2".to_string(), 0.80f32)];
        let hits3 = vec![("id1".to_string(), 0.95f32), ("id3".to_string(), 0.80f32)];

        assert_eq!(compute_result_hash(&hits1), compute_result_hash(&hits2));
        assert_ne!(compute_result_hash(&hits1), compute_result_hash(&hits3));
    }
}

