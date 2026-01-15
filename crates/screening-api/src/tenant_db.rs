use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

use crate::tenant::Tenant;

/// Initialize tenant tables in SQLite
pub fn init_tenant_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS tenant (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            api_key_hash TEXT NOT NULL,
            is_active INTEGER NOT NULL DEFAULT 1,
            hit_threshold REAL NOT NULL DEFAULT 0.9,
            review_threshold REAL NOT NULL DEFAULT 0.75,
            rate_limit_per_minute INTEGER NOT NULL DEFAULT 1000,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_tenant_active ON tenant(is_active);

        CREATE TABLE IF NOT EXISTS api_key (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL REFERENCES tenant(id),
            key_hash TEXT NOT NULL,
            name TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            last_used_at TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_api_key_hash ON api_key(key_hash);
        CREATE INDEX IF NOT EXISTS idx_api_key_tenant ON api_key(tenant_id, is_active);

        CREATE TABLE IF NOT EXISTS usage_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            tenant_id TEXT NOT NULL,
            api_key_id TEXT,
            endpoint TEXT NOT NULL,
            request_count INTEGER NOT NULL DEFAULT 1,
            timestamp TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_usage_tenant_time 
            ON usage_log(tenant_id, timestamp DESC);
        "
    )?;

    tracing::info!("tenant schema initialized");
    Ok(())
}

/// Open or create the tenant database
pub fn open_tenant_db(data_dir: &str) -> Result<Connection> {
    let db_path = Path::new(data_dir).join("tenants.db");
    
    // Ensure directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    
    init_tenant_schema(&conn)?;
    
    Ok(conn)
}

/// Hash an API key for storage
pub fn hash_api_key(key: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Create a new tenant
pub fn create_tenant(
    conn: &Connection,
    id: &str,
    name: &str,
    api_key: &str,
) -> Result<()> {
    let key_hash = hash_api_key(api_key);
    
    conn.execute(
        "INSERT INTO tenant (id, name, api_key_hash) VALUES (?1, ?2, ?3)",
        rusqlite::params![id, name, key_hash],
    )?;

    // Also create the API key entry
    let key_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO api_key (id, tenant_id, key_hash, name) VALUES (?1, ?2, ?3, 'default')",
        rusqlite::params![key_id, id, key_hash],
    )?;

    tracing::info!(tenant_id = id, name, "created tenant");
    Ok(())
}

/// Get tenant by API key
pub fn get_tenant_by_key(conn: &Connection, api_key: &str) -> Option<Tenant> {
    let key_hash = hash_api_key(api_key);
    
    let result = conn.query_row(
        "SELECT t.id, t.name, ?1, t.is_active, t.hit_threshold, t.review_threshold, t.rate_limit_per_minute
         FROM tenant t
         JOIN api_key k ON t.id = k.tenant_id
         WHERE k.key_hash = ?2 AND k.is_active = 1 AND t.is_active = 1",
        rusqlite::params![api_key, key_hash],
        |row| {
            Ok(Tenant {
                id: row.get(0)?,
                name: row.get(1)?,
                api_key: row.get(2)?,
                is_active: row.get::<_, i32>(3)? == 1,
                hit_threshold: row.get(4)?,
                review_threshold: row.get(5)?,
                rate_limit_per_minute: row.get(6)?,
            })
        },
    );

    match result {
        Ok(tenant) => {
            // Update last_used_at
            let _ = conn.execute(
                "UPDATE api_key SET last_used_at = datetime('now') WHERE key_hash = ?1",
                [&key_hash],
            );
            Some(tenant)
        }
        Err(_) => None,
    }
}

/// Get tenant by ID
pub fn get_tenant(conn: &Connection, tenant_id: &str) -> Option<Tenant> {
    conn.query_row(
        "SELECT id, name, api_key_hash, is_active, hit_threshold, review_threshold, rate_limit_per_minute
         FROM tenant WHERE id = ?1",
        [tenant_id],
        |row| {
            Ok(Tenant {
                id: row.get(0)?,
                name: row.get(1)?,
                api_key: String::new(), // Don't expose the key
                is_active: row.get::<_, i32>(3)? == 1,
                hit_threshold: row.get(4)?,
                review_threshold: row.get(5)?,
                rate_limit_per_minute: row.get(6)?,
            })
        },
    ).ok()
}

/// List all tenants
pub fn list_tenants(conn: &Connection) -> Result<Vec<Tenant>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, '', is_active, hit_threshold, review_threshold, rate_limit_per_minute
         FROM tenant ORDER BY name"
    )?;

    let tenants = stmt.query_map([], |row| {
        Ok(Tenant {
            id: row.get(0)?,
            name: row.get(1)?,
            api_key: String::new(),
            is_active: row.get::<_, i32>(3)? == 1,
            hit_threshold: row.get(4)?,
            review_threshold: row.get(5)?,
            rate_limit_per_minute: row.get(6)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(tenants)
}

/// Update tenant settings
pub fn update_tenant(
    conn: &Connection,
    tenant_id: &str,
    name: Option<&str>,
    is_active: Option<bool>,
    hit_threshold: Option<f32>,
    review_threshold: Option<f32>,
    rate_limit: Option<u32>,
) -> Result<bool> {
    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    
    if let Some(n) = name {
        updates.push("name = ?");
        params.push(Box::new(n.to_string()));
    }
    if let Some(a) = is_active {
        updates.push("is_active = ?");
        params.push(Box::new(a as i32));
    }
    if let Some(h) = hit_threshold {
        updates.push("hit_threshold = ?");
        params.push(Box::new(h));
    }
    if let Some(r) = review_threshold {
        updates.push("review_threshold = ?");
        params.push(Box::new(r));
    }
    if let Some(l) = rate_limit {
        updates.push("rate_limit_per_minute = ?");
        params.push(Box::new(l as i32));
    }

    if updates.is_empty() {
        return Ok(false);
    }

    updates.push("updated_at = datetime('now')");
    params.push(Box::new(tenant_id.to_string()));

    let sql = format!(
        "UPDATE tenant SET {} WHERE id = ?",
        updates.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = conn.execute(&sql, params_refs.as_slice())?;
    
    Ok(rows > 0)
}

/// Create a new API key for a tenant
pub fn create_api_key(
    conn: &Connection,
    tenant_id: &str,
    name: Option<&str>,
    expires_at: Option<&str>,
) -> Result<String> {
    use rand::Rng;
    
    // Generate a random API key
    let mut rng = rand::thread_rng();
    let bytes: [u8; 24] = rng.gen();
    let key = format!("ak_{}", base64_encode(&bytes));
    let key_hash = hash_api_key(&key);
    
    let key_id = uuid::Uuid::new_v4().to_string();
    
    conn.execute(
        "INSERT INTO api_key (id, tenant_id, key_hash, name, expires_at) 
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![key_id, tenant_id, key_hash, name, expires_at],
    )?;

    tracing::info!(tenant_id, key_id, "created new API key");
    Ok(key)
}

/// Revoke an API key
pub fn revoke_api_key(conn: &Connection, key_id: &str) -> Result<bool> {
    let rows = conn.execute(
        "UPDATE api_key SET is_active = 0 WHERE id = ?1",
        [key_id],
    )?;
    Ok(rows > 0)
}

/// Log API usage
pub fn log_usage(
    conn: &Connection,
    tenant_id: &str,
    api_key_id: Option<&str>,
    endpoint: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_log (tenant_id, api_key_id, endpoint) VALUES (?1, ?2, ?3)",
        rusqlite::params![tenant_id, api_key_id, endpoint],
    )?;
    Ok(())
}

/// Get usage stats for a tenant
pub fn get_usage_stats(
    conn: &Connection,
    tenant_id: &str,
    since: &str,
) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT endpoint, SUM(request_count) as count
         FROM usage_log 
         WHERE tenant_id = ?1 AND timestamp >= ?2
         GROUP BY endpoint"
    )?;

    let stats = stmt.query_map(rusqlite::params![tenant_id, since], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?.collect::<Result<Vec<_>, _>>()?;

    Ok(stats)
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let mut n = 0u32;
        for (i, &byte) in chunk.iter().enumerate() {
            n |= (byte as u32) << (16 - 8 * i);
        }
        let padding = 3 - chunk.len();
        for i in 0..(4 - padding) {
            let idx = ((n >> (18 - 6 * i)) & 0x3F) as usize;
            result.push(ALPHABET[idx] as char);
        }
    }
    result
}

/// Ensure default tenant exists
pub fn ensure_default_tenant(conn: &Connection) -> Result<()> {
    let exists = conn.query_row(
        "SELECT 1 FROM tenant WHERE id = 'default'",
        [],
        |_| Ok(true),
    ).unwrap_or(false);

    if !exists {
        create_tenant(conn, "default", "Default Tenant", "test-api-key")?;
        tracing::info!("created default tenant with key 'test-api-key'");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn tenant_crud() {
        let dir = tempdir().unwrap();
        let conn = open_tenant_db(dir.path().to_str().unwrap()).unwrap();

        // Create tenant
        create_tenant(&conn, "t1", "Test Tenant", "my-secret-key").unwrap();

        // Get by key
        let tenant = get_tenant_by_key(&conn, "my-secret-key").unwrap();
        assert_eq!(tenant.id, "t1");
        assert_eq!(tenant.name, "Test Tenant");

        // Invalid key
        assert!(get_tenant_by_key(&conn, "wrong-key").is_none());

        // List tenants
        let tenants = list_tenants(&conn).unwrap();
        assert_eq!(tenants.len(), 1);

        // Update
        update_tenant(&conn, "t1", Some("New Name"), None, None, None, None).unwrap();
        let tenant = get_tenant(&conn, "t1").unwrap();
        assert_eq!(tenant.name, "New Name");
    }

    #[test]
    fn api_key_management() {
        let dir = tempdir().unwrap();
        let conn = open_tenant_db(dir.path().to_str().unwrap()).unwrap();

        create_tenant(&conn, "t1", "Test", "initial-key").unwrap();

        // Create new key
        let new_key = create_api_key(&conn, "t1", Some("Production"), None).unwrap();
        assert!(new_key.starts_with("ak_"));

        // Both keys should work
        assert!(get_tenant_by_key(&conn, "initial-key").is_some());
        assert!(get_tenant_by_key(&conn, &new_key).is_some());
    }
}



