use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookSubscription {
    pub id: String,
    pub tenant_id: String,
    pub url: String,
    pub events: Vec<String>,
    pub secret: String,
    pub created_at: String,
    pub active: bool,
}

#[derive(Debug, Serialize)]
pub struct WebhookEvent {
    pub id: String,
    pub event_type: String,
    pub timestamp: String,
    pub data: serde_json::Value,
}

pub struct WebhookStore {
    conn: Arc<Mutex<Connection>>,
}

impl WebhookStore {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    pub fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS webhook_subscription (
                id TEXT PRIMARY KEY,
                tenant_id TEXT NOT NULL,
                url TEXT NOT NULL,
                events TEXT NOT NULL,
                secret TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                active INTEGER NOT NULL DEFAULT 1
            )
            "#,
            [],
        )?;
        Ok(())
    }

    pub async fn create_subscription(
        &self,
        tenant_id: &str,
        url: &str,
        events: Vec<String>,
    ) -> Result<WebhookSubscription> {
        let id = uuid::Uuid::new_v4().to_string();
        let secret = generate_secret();
        let events_json = serde_json::to_string(&events)?;
        
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO webhook_subscription (id, tenant_id, url, events, secret, created_at, active) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            rusqlite::params![id, tenant_id, url, events_json, secret, Utc::now().to_rfc3339()],
        )?;
        
        Ok(WebhookSubscription {
            id,
            tenant_id: tenant_id.to_string(),
            url: url.to_string(),
            events,
            secret,
            created_at: Utc::now().to_rfc3339(),
            active: true,
        })
    }

    pub async fn get_active_subscriptions(&self, tenant_id: &str, event_type: &str) -> Result<Vec<WebhookSubscription>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, url, events, secret, created_at, active FROM webhook_subscription WHERE tenant_id = ?1 AND active = 1"
        )?;
        
        let rows = stmt.query_map([tenant_id], |row| {
            let events_json: String = row.get(3)?;
            let events: Vec<String> = serde_json::from_str(&events_json).unwrap_or_default();
            
            Ok(WebhookSubscription {
                id: row.get(0)?,
                tenant_id: row.get(1)?,
                url: row.get(2)?,
                events,
                secret: row.get(4)?,
                created_at: row.get(5)?,
                active: row.get::<_, i32>(6)? != 0,
            })
        })?;
        
        let mut subscriptions = Vec::new();
        for row in rows {
            let sub = row?;
            if sub.events.contains(&event_type.to_string()) || sub.events.contains(&"*".to_string()) {
                subscriptions.push(sub);
            }
        }
        
        Ok(subscriptions)
    }

    pub async fn delete_subscription(&self, subscription_id: &str) -> Result<bool> {
        let conn = self.conn.lock().await;
        let rows = conn.execute(
            "DELETE FROM webhook_subscription WHERE id = ?1",
            [subscription_id],
        )?;
        Ok(rows > 0)
    }
}

fn generate_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    hex::encode(bytes)
}

pub async fn deliver_webhook(
    subscription: &WebhookSubscription,
    event: &WebhookEvent,
) -> Result<()> {
    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "id": event.id,
        "event_type": event.event_type,
        "timestamp": event.timestamp,
        "data": event.data,
    });
    
    let signature = compute_signature(&subscription.secret, &payload.to_string())?;
    
    let response = client
        .post(&subscription.url)
        .header("Content-Type", "application/json")
        .header("X-Aegistry-Signature", signature)
        .json(&payload)
        .send()
        .await?;
    
    if !response.status().is_success() {
        anyhow::bail!("Webhook delivery failed with status {}", response.status());
    }
    
    Ok(())
}

fn compute_signature(secret: &str, payload: &str) -> Result<String> {
    use sha2::{Sha256, Digest};
    
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(payload.as_bytes());
    let result = hasher.finalize();
    Ok(hex::encode(result))
}

