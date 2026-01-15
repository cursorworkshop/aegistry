use aegistry_core::Hit;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{perform_screening, AppState};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
pub enum BatchStatus {
    Processing,
    Completed,
    Failed,
}

#[derive(Clone, Debug)]
pub struct BatchJob {
    pub id: String,
    pub status: BatchStatus,
    pub total_records: usize,
    pub processed_records: usize,
    pub created_at: String,
    pub results: Vec<BatchResult>,
}

impl BatchJob {
    pub fn new(id: String, total_records: usize) -> Self {
        Self {
            id,
            status: BatchStatus::Processing,
            total_records,
            processed_records: 0,
            created_at: Utc::now().to_rfc3339(),
            results: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BatchRequest {
    pub records: Vec<BatchRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BatchRecord {
    pub reference_id: Option<String>,
    pub name: String,
    pub country: Option<String>,
    pub date_of_birth: Option<String>,
    #[serde(default)]
    pub record_type: RecordType,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, ToSchema)]
pub enum RecordType {
    #[default]
    Person,
    Entity,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct BatchResponse {
    pub job_id: String,
    pub status: BatchStatus,
    pub total_records: usize,
    pub processed_records: usize,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct BatchResult {
    pub reference_id: Option<String>,
    pub name: String,
    pub hits: Vec<Hit>,
    pub checked_at: String,
}

pub async fn process_batch(state: AppState, job_id: String, records: Vec<BatchRecord>) {
    let mut results = Vec::new();

    for (idx, record) in records.iter().enumerate() {
        let dob_year = record
            .date_of_birth
            .as_ref()
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse::<i32>().ok());

        let hits = perform_screening(&state, &record.name, record.country.as_deref(), dob_year);

        results.push(BatchResult {
            reference_id: record.reference_id.clone(),
            name: record.name.clone(),
            hits,
            checked_at: Utc::now().to_rfc3339(),
        });

        // Update progress
        {
            let mut jobs = state.batch_jobs.write().await;
            if let Some(job) = jobs.get_mut(&job_id) {
                job.processed_records = idx + 1;
            }
        }
    }

    // Mark as completed
    {
        let mut jobs = state.batch_jobs.write().await;
        if let Some(job) = jobs.get_mut(&job_id) {
            job.status = BatchStatus::Completed;
            job.results = results;
        }
    }
}

