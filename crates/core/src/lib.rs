use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

pub const PROJECT_NAME: &str = "aegistry";
pub const PROJECT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthStatus<'a> {
    pub status: &'a str,
    pub service: &'a str,
    pub version: &'a str,
}

pub fn health_status(service: &'static str) -> HealthStatus<'static> {
    HealthStatus {
        status: "ok",
        service,
        version: PROJECT_VERSION,
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ScreenPersonRequest {
    pub reference_id: Option<String>,
    #[validate(length(min = 1))]
    pub first_name: String,
    #[validate(length(min = 1))]
    pub last_name: String,
    pub date_of_birth: Option<String>,
    #[validate(length(min = 2, max = 2))]
    pub country: Option<String>,
    #[validate(length(min = 2, max = 2))]
    pub nationality: Option<String>,
}

impl ScreenPersonRequest {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name.trim(), self.last_name.trim())
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn dob_year(&self) -> Option<i32> {
        self.date_of_birth
            .as_ref()
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse::<i32>().ok())
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScreenPersonResponse {
    pub request_id: String,
    pub reference_id: Option<String>,
    pub hits: Vec<Hit>,
    pub checked_at: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Hit {
    pub subject_id: String,
    pub matched_name: String,
    pub source: HitSource,
    pub kind: SubjectKind,
    pub score: f32,
    pub risk_level: RiskLevel,
    pub components: ScoreComponents,
    pub explanation: Vec<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct ScoreComponents {
    pub name_similarity: f32,
    pub dob_similarity: f32,
    pub country_match: f32,
}

impl ScoreComponents {
    pub fn explain(&self, matched_name: &str, country: Option<&str>) -> Vec<String> {
        let mut explanations = Vec::new();
        
        if self.name_similarity >= 0.95 {
            explanations.push(format!("Name '{}' is a very close match ({:.0}%)", matched_name, self.name_similarity * 100.0));
        } else if self.name_similarity >= 0.8 {
            explanations.push(format!("Name '{}' is similar ({:.0}%)", matched_name, self.name_similarity * 100.0));
        } else {
            explanations.push(format!("Name '{}' partially matches ({:.0}%)", matched_name, self.name_similarity * 100.0));
        }
        
        if self.country_match > 0.0 {
            if let Some(c) = country {
                explanations.push(format!("Country '{}' matches", c));
            }
        }
        
        if self.dob_similarity >= 1.0 {
            explanations.push("Date of birth matches exactly".to_string());
        } else if self.dob_similarity >= 0.5 {
            explanations.push("Year of birth is close".to_string());
        }
        
        explanations
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub enum RiskLevel {
    Hit,
    Review,
    None,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub enum SubjectKind {
    Person,
    Entity,
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
pub enum HitSource {
    EuConsolidated,
    UnSc,
    Ofac,
    Uk,
    PepEu,
    Stub,
}

pub fn new_request_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VersionResponse<'a> {
    pub service: &'a str,
    pub project: &'a str,
    pub version: &'a str,
}
