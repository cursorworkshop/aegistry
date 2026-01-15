use anyhow::{Context, Result};
use std::time::Duration;

use crate::parser_eu::{ParsedSubject, SubjectKind};

const BUNDESTAG_API_URL: &str = "https://www.bundestag.de/api";
const BUNDESTAG_MEMBERS_URL: &str = "https://www.bundestag.de/abgeordnete";

/// Fetch German Bundestag members from official sources
pub async fn fetch_german_bundestag() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")?;

    tracing::info!("fetching German Bundestag members");

    // Try API first
    let api_url = format!("{}/members", BUNDESTAG_API_URL);
    let response = client.get(&api_url).send().await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await?;
            parse_bundestag_api(&text)
        }
        _ => {
            // Fallback: scrape HTML
            tracing::info!("API failed, scraping Bundestag HTML");
            fetch_bundestag_html(&client).await
        }
    }
}

fn parse_bundestag_api(json: &str) -> Result<Vec<ParsedSubject>> {
    // Try to parse JSON response
    use serde_json::Value;
    
    let mut members = Vec::new();
    
    if let Ok(data) = serde_json::from_str::<Value>(json) {
        if let Some(items) = data.as_array() {
            for item in items {
                if let Some(name) = item.get("name").and_then(|v| v.as_str())
                    .or_else(|| item.get("fullName").and_then(|v| v.as_str()))
                {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_i64())
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| {
                            name.chars().filter(|c| c.is_alphanumeric()).take(20).collect()
                        });
                    
                    members.push(ParsedSubject {
                        source_ref: format!("pep_de_bundestag_{}", id),
                        kind: SubjectKind::Person,
                        primary_name: name.to_string(),
                        aliases: Vec::new(),
                        date_of_birth: None,
                        date_of_birth_year: None,
                        country: Some("DE".to_string()),
                        nationalities: vec!["DE".to_string()],
                    });
                }
            }
        }
    }
    
    Ok(members)
}

async fn fetch_bundestag_html(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    let response = client
        .get(BUNDESTAG_MEMBERS_URL)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch Bundestag HTML")?;

    if !response.status().is_success() {
        anyhow::bail!("Bundestag page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let members = parse_bundestag_html(&html);
    
    Ok(members)
}

fn parse_bundestag_html(html: &str) -> Vec<ParsedSubject> {
    let mut members = Vec::new();
    
    // Parse HTML to extract member names
    use regex::Regex;
    
    let name_patterns = [
        r#"<a[^>]*href="/abgeordnete/[^"]*"[^>]*>([^<]+)</a>"#,
        r#"<span[^>]*class="[^"]*name[^"]*"[^>]*>([^<]+)</span>"#,
        r#"data-name="([^"]+)"#,
        r#"<h[23][^>]*>([A-ZÄÖÜ][a-zäöüß]+ [A-ZÄÖÜ][a-zäöüß]+)</h[23]>"#,
    ];

    for pattern in &name_patterns {
        if let Ok(re) = Regex::new(pattern) {
            for cap in re.captures_iter(html) {
                if let Some(name_match) = cap.get(1) {
                    let name = name_match.as_str().trim().to_string();
                    if name.len() > 5 && name.contains(' ') && !name.contains("Abgeordnete") {
                        let id = name
                            .chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(20)
                            .collect::<String>();
                        
                        // Check if already added
                        if !members.iter().any(|m: &ParsedSubject| m.primary_name == name) {
                            members.push(ParsedSubject {
                                source_ref: format!("pep_de_bundestag_{}", id),
                                kind: SubjectKind::Person,
                                primary_name: name,
                                aliases: Vec::new(),
                                date_of_birth: None,
                                date_of_birth_year: None,
                                country: Some("DE".to_string()),
                                nationalities: vec!["DE".to_string()],
                            });
                        }
                    }
                }
            }
        }
    }
    
    members
}

