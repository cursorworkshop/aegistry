use anyhow::{Context, Result};
use std::time::Duration;
use crate::parser_eu::{ParsedSubject, SubjectKind};

pub async fn fetch_spain_congress() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0")
        .build()
        .context("failed to build HTTP client")?;
    
    tracing::info!("fetching Spanish Congress members");
    
    let url = "https://www.congreso.es/web/guest/diputados";
    let html = client.get(url).send().await?.text().await?;
    
    let mut members = Vec::new();
    let re = regex::Regex::new(r#"([A-ZÁÉÍÓÚÑ][a-záéíóúñ]+ [A-ZÁÉÍÓÚÑ][a-záéíóúñ]+)"#)
        .context("failed to compile regex")?;
    
    for cap in re.captures_iter(&html) {
        if let Some(name) = cap.get(1) {
            let name = name.as_str().trim().to_string();
            if name.len() > 5 && !members.iter().any(|m: &ParsedSubject| m.primary_name == name) {
                let id = name
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .take(20)
                    .collect::<String>();
                members.push(ParsedSubject {
                    source_ref: format!("pep_es_{}", id.to_lowercase()),
                    kind: SubjectKind::Person,
                    primary_name: name,
                    aliases: Vec::new(),
                    date_of_birth: None,
                    date_of_birth_year: None,
                    country: Some("ES".to_string()),
                    nationalities: vec!["ES".to_string()],
                });
            }
        }
    }
    
    tracing::info!(count = members.len(), "parsed Spanish Congress members");
    Ok(members)
}

