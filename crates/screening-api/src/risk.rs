use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    pub tenant_id: String,
    pub hit_threshold: f32,
    pub review_threshold: f32,
    pub name_weight: f32,
    pub dob_weight: f32,
    pub country_weight: f32,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            tenant_id: String::new(),
            hit_threshold: 0.95,
            review_threshold: 0.90,
            name_weight: 0.70,
            dob_weight: 0.10,
            country_weight: 0.20,
        }
    }
}

pub struct RiskStore {
    conn: Arc<Mutex<Connection>>,
}

impl RiskStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS risk_config (
                tenant_id TEXT PRIMARY KEY,
                hit_threshold REAL NOT NULL DEFAULT 0.95,
                review_threshold REAL NOT NULL DEFAULT 0.90,
                name_weight REAL NOT NULL DEFAULT 0.70,
                dob_weight REAL NOT NULL DEFAULT 0.10,
                country_weight REAL NOT NULL DEFAULT 0.20,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
            [],
        )?;
        Ok(())
    }

    pub async fn get_config(&self, tenant_id: &str) -> Result<RiskConfig> {
        let conn = self.conn.lock().await;
        match conn.query_row(
            "SELECT tenant_id, hit_threshold, review_threshold, name_weight, dob_weight, country_weight FROM risk_config WHERE tenant_id = ?1",
            [tenant_id],
            |row| {
                Ok(RiskConfig {
                    tenant_id: row.get(0)?,
                    hit_threshold: row.get(1)?,
                    review_threshold: row.get(2)?,
                    name_weight: row.get(3)?,
                    dob_weight: row.get(4)?,
                    country_weight: row.get(5)?,
                })
            },
        ) {
            Ok(config) => Ok(config),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Return default config
                Ok(RiskConfig {
                    tenant_id: tenant_id.to_string(),
                    ..Default::default()
                })
            }
            Err(e) => Err(anyhow::Error::from(e)),
        }
    }

    pub async fn set_config(&self, config: &RiskConfig) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT OR REPLACE INTO risk_config (tenant_id, hit_threshold, review_threshold, name_weight, dob_weight, country_weight, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
            rusqlite::params![
                config.tenant_id,
                config.hit_threshold,
                config.review_threshold,
                config.name_weight,
                config.dob_weight,
                config.country_weight,
            ],
        )?;
        Ok(())
    }
}



