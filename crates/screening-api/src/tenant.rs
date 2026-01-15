use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Clone, Debug)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub api_key: String,
    pub is_active: bool,
    pub hit_threshold: f32,
    pub review_threshold: f32,
    pub rate_limit_per_minute: u32,
}

impl Default for Tenant {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Default Tenant".to_string(),
            api_key: "test-api-key".to_string(),
            is_active: true,
            hit_threshold: 0.9,
            review_threshold: 0.75,
            rate_limit_per_minute: 1000,
        }
    }
}

pub struct TenantStore {
    tenants: RwLock<HashMap<String, Tenant>>,
    api_key_index: RwLock<HashMap<String, String>>, // api_key -> tenant_id
}

impl TenantStore {
    pub fn new(_data_dir: &str) -> Self {
        Self {
            tenants: RwLock::new(HashMap::new()),
            api_key_index: RwLock::new(HashMap::new()),
        }
    }

    pub fn create_default_tenant(&self) {
        let tenant = Tenant::default();
        self.add_tenant(tenant);
    }

    pub fn add_tenant(&self, tenant: Tenant) {
        let api_key = tenant.api_key.clone();
        let tenant_id = tenant.id.clone();
        
        {
            let mut tenants = self.tenants.write().unwrap();
            tenants.insert(tenant_id.clone(), tenant);
        }
        {
            let mut index = self.api_key_index.write().unwrap();
            index.insert(api_key, tenant_id);
        }
    }

    pub fn get_tenant_by_key(&self, api_key: &str) -> Option<Tenant> {
        let tenant_id = {
            let index = self.api_key_index.read().unwrap();
            index.get(api_key).cloned()
        };

        tenant_id.and_then(|id| {
            let tenants = self.tenants.read().unwrap();
            tenants.get(&id).cloned()
        })
    }

    pub fn get_tenant(&self, tenant_id: &str) -> Option<Tenant> {
        let tenants = self.tenants.read().unwrap();
        tenants.get(tenant_id).cloned()
    }

    pub fn generate_api_key() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let bytes: [u8; 24] = rng.gen();
        format!("ak_{}", base64_encode(&bytes))
    }
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



