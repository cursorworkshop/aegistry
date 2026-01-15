use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct ScreeningStats {
    pub total_screenings: i64,
    pub hits: i64,
    pub reviews: i64,
    pub none: i64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct SourceDistribution {
    pub source: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct PerformanceMetrics {
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
}

pub struct AnalyticsStore {
    conn: Arc<Mutex<Connection>>,
}

impl AnalyticsStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS screening_metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tenant_id TEXT NOT NULL,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                latency_ms REAL NOT NULL,
                risk_level TEXT NOT NULL,
                source TEXT
            )
            "#,
            [],
        )?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_metrics_tenant ON screening_metrics(tenant_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_metrics_timestamp ON screening_metrics(timestamp)",
            [],
        )?;
        
        Ok(())
    }

    pub async fn record_screening(
        &self,
        tenant_id: &str,
        latency_ms: f64,
        risk_level: &str,
        source: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO screening_metrics (tenant_id, latency_ms, risk_level, source) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![tenant_id, latency_ms, risk_level, source],
        )?;
        Ok(())
    }

    pub async fn get_screening_stats(&self, tenant_id: &str, days: i32) -> Result<ScreeningStats> {
        let conn = self.conn.lock().await;
        
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM screening_metrics WHERE tenant_id = ?1 AND timestamp > datetime('now', '-' || ?2 || ' days')",
            [tenant_id, &days.to_string()],
            |row| row.get(0),
        )?;
        
        let hits: i64 = conn.query_row(
            "SELECT COUNT(*) FROM screening_metrics WHERE tenant_id = ?1 AND risk_level = 'Hit' AND timestamp > datetime('now', '-' || ?2 || ' days')",
            [tenant_id, &days.to_string()],
            |row| row.get(0),
        )?;
        
        let reviews: i64 = conn.query_row(
            "SELECT COUNT(*) FROM screening_metrics WHERE tenant_id = ?1 AND risk_level = 'Review' AND timestamp > datetime('now', '-' || ?2 || ' days')",
            [tenant_id, &days.to_string()],
            |row| row.get(0),
        )?;
        
        let none: i64 = conn.query_row(
            "SELECT COUNT(*) FROM screening_metrics WHERE tenant_id = ?1 AND risk_level = 'None' AND timestamp > datetime('now', '-' || ?2 || ' days')",
            [tenant_id, &days.to_string()],
            |row| row.get(0),
        )?;
        
        let avg_latency: f64 = conn.query_row(
            "SELECT AVG(latency_ms) FROM screening_metrics WHERE tenant_id = ?1 AND timestamp > datetime('now', '-' || ?2 || ' days')",
            [tenant_id, &days.to_string()],
            |row| row.get::<_, Option<f64>>(0),
        ).ok().flatten().unwrap_or(0.0);
        
        // P95 approximation: get 95th percentile
        let p95_latency: f64 = conn.query_row(
            "SELECT latency_ms FROM screening_metrics WHERE tenant_id = ?1 AND timestamp > datetime('now', '-' || ?2 || ' days') ORDER BY latency_ms LIMIT 1 OFFSET CAST(COUNT(*) * 0.95 AS INTEGER)",
            [tenant_id, &days.to_string()],
            |row| row.get::<_, Option<f64>>(0),
        ).ok().flatten().unwrap_or(0.0);
        
        Ok(ScreeningStats {
            total_screenings: total,
            hits,
            reviews,
            none,
            avg_latency_ms: avg_latency,
            p95_latency_ms: p95_latency,
        })
    }

    pub async fn get_source_distribution(&self, tenant_id: &str, days: i32) -> Result<Vec<SourceDistribution>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) as count FROM screening_metrics WHERE tenant_id = ?1 AND timestamp > datetime('now', '-' || ?2 || ' days') AND source IS NOT NULL GROUP BY source ORDER BY count DESC"
        )?;
        
        let rows = stmt.query_map([tenant_id, &days.to_string()], |row| {
            Ok(SourceDistribution {
                source: row.get(0)?,
                count: row.get(1)?,
            })
        })?;
        
        let mut distributions = Vec::new();
        for row in rows {
            distributions.push(row?);
        }
        Ok(distributions)
    }
}



