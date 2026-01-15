use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    pub tenant_id: String,
    pub user_id: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub timestamp: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub details: Option<String>,
}

pub struct AuditStore {
    conn: Arc<Mutex<Connection>>,
}

impl AuditStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                user_id TEXT,
                action TEXT NOT NULL,
                resource_type TEXT NOT NULL,
                resource_id TEXT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                ip_address TEXT,
                user_agent TEXT,
                details TEXT
            )
            "#,
            [],
        )?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_tenant ON audit_log(tenant_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log(action)",
            [],
        )?;
        
        Ok(())
    }

    pub async fn log(
        &self,
        tenant_id: &str,
        action: &str,
        resource_type: &str,
        resource_id: Option<&str>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
        details: Option<&str>,
    ) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO audit_log (id, tenant_id, action, resource_type, resource_id, timestamp, ip_address, user_agent, details) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                id,
                tenant_id,
                action,
                resource_type,
                resource_id,
                Utc::now().to_rfc3339(),
                ip_address,
                user_agent,
                details
            ],
        )?;
        Ok(())
    }

    pub async fn query(
        &self,
        tenant_id: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AuditLog>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, user_id, action, resource_type, resource_id, timestamp, ip_address, user_agent, details FROM audit_log WHERE tenant_id = ?1 ORDER BY timestamp DESC LIMIT ?2 OFFSET ?3"
        )?;
        
        let rows = stmt.query_map([tenant_id, &limit.to_string(), &offset.to_string()], |row| {
            Ok(AuditLog {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                user_id: row.get(2)?,
                action: row.get(3)?,
                resource_type: row.get(4)?,
                resource_id: row.get(5)?,
                timestamp: row.get(6)?,
                ip_address: row.get(7)?,
                user_agent: row.get(8)?,
                details: row.get(9)?,
            })
        })?;
        
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }
        Ok(logs)
    }
}



