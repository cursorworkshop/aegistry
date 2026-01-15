use anyhow::{Context, Result};
use std::time::Duration;

use crate::parser_eu::{ParsedSubject, SubjectKind};

const UK_PARLIAMENT_API_URL: &str = "https://members-api.parliament.uk/api";
const UK_PARLIAMENT_MEMBERS_URL: &str = "https://members.parliament.uk/members/commons";

/// Fetch UK Parliament members (House of Commons + Lords) from official sources
pub async fn fetch_uk_parliament() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")?;

    tracing::info!("fetching UK Parliament members");

    let mut all_members = Vec::new();

    // Fetch House of Commons
    match fetch_commons_members(&client).await {
        Ok(members) => {
            tracing::info!(count = members.len(), "fetched Commons members");
            all_members.extend(members);
        }
        Err(e) => tracing::warn!(error = %e, "failed to fetch Commons members"),
    }

    // Fetch House of Lords
    match fetch_lords_members(&client).await {
        Ok(members) => {
            tracing::info!(count = members.len(), "fetched Lords members");
            all_members.extend(members);
        }
        Err(e) => tracing::warn!(error = %e, "failed to fetch Lords members"),
    }

    // Deduplicate by name
    all_members.sort_by(|a, b| a.primary_name.cmp(&b.primary_name));
    all_members.dedup_by(|a, b| a.primary_name == b.primary_name);

    tracing::info!(total = all_members.len(), "total UK Parliament members");
    Ok(all_members)
}

async fn fetch_commons_members(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    // Try official API first
    let api_url = format!("{}/Members/Search", UK_PARLIAMENT_API_URL);
    let response = client
        .get(&api_url)
        .query(&[("house", "Commons"), ("skip", "0"), ("take", "1000")])
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await?;
            parse_uk_parliament_api(&text, "commons")
        }
        _ => {
            // Fallback: scrape HTML
            tracing::info!("API failed, scraping Commons HTML");
            fetch_commons_html(client).await
        }
    }
}

async fn fetch_lords_members(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    // Try official API first
    let api_url = format!("{}/Members/Search", UK_PARLIAMENT_API_URL);
    let response = client
        .get(&api_url)
        .query(&[("house", "Lords"), ("skip", "0"), ("take", "1000")])
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await?;
            parse_uk_parliament_api(&text, "lords")
        }
        _ => {
            // Fallback: scrape HTML
            tracing::info!("API failed, scraping Lords HTML");
            fetch_lords_html(client).await
        }
    }
}

fn parse_uk_parliament_api(json: &str, chamber: &str) -> Result<Vec<ParsedSubject>> {
    // Try to parse JSON response
    use serde_json::Value;
    
    let mut members = Vec::new();
    
    if let Ok(data) = serde_json::from_str::<Value>(json) {
        if let Some(items) = data.get("items").and_then(|v| v.as_array()) {
            for item in items {
                if let Some(name) = item.get("nameDisplayAs").and_then(|v| v.as_str()) {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_i64())
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| {
                            name.chars().filter(|c| c.is_alphanumeric()).take(20).collect()
                        });
                    
                    members.push(ParsedSubject {
                        source_ref: format!("pep_uk_{}_{}", chamber, id),
                        kind: SubjectKind::Person,
                        primary_name: name.to_string(),
                        aliases: Vec::new(),
                        date_of_birth: None,
                        date_of_birth_year: None,
                        country: Some("GB".to_string()),
                        nationalities: vec!["GB".to_string()],
                    });
                }
            }
        }
    }
    
    Ok(members)
}

async fn fetch_commons_html(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    let response = client
        .get(UK_PARLIAMENT_MEMBERS_URL)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch Commons HTML")?;

    if !response.status().is_success() {
        anyhow::bail!("Commons page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let members = parse_uk_parliament_html(&html, "commons");
    
    Ok(members)
}

async fn fetch_lords_html(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    let url = "https://members.parliament.uk/members/lords";
    let response = client
        .get(url)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch Lords HTML")?;

    if !response.status().is_success() {
        anyhow::bail!("Lords page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let members = parse_uk_parliament_html(&html, "lords");
    
    Ok(members)
}

fn parse_uk_parliament_html(html: &str, chamber: &str) -> Vec<ParsedSubject> {
    let mut members = Vec::new();
    
    // Parse HTML to extract member names
    use regex::Regex;
    
    let name_patterns = [
        r#"<a[^>]*href="/members/[^"]*"[^>]*>([^<]+)</a>"#,
        r#"<span[^>]*class="[^"]*name[^"]*"[^>]*>([^<]+)</span>"#,
        r#"data-member-name="([^"]+)"#,
    ];

    for pattern in &name_patterns {
        if let Ok(re) = Regex::new(pattern) {
            for cap in re.captures_iter(html) {
                if let Some(name_match) = cap.get(1) {
                    let name = name_match.as_str().trim().to_string();
                    if name.len() > 5 && name.contains(' ') && !name.contains("MP") && !name.contains("Lord") {
                        let id = name
                            .chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(20)
                            .collect::<String>();
                        
                        // Check if already added
                        if !members.iter().any(|m: &ParsedSubject| m.primary_name == name) {
                            members.push(ParsedSubject {
                                source_ref: format!("pep_uk_{}_{}", chamber, id),
                                kind: SubjectKind::Person,
                                primary_name: name,
                                aliases: Vec::new(),
                                date_of_birth: None,
                                date_of_birth_year: None,
                                country: Some("GB".to_string()),
                                nationalities: vec!["GB".to_string()],
                            });
                        }
                    }
                }
            }
        }
    }
    
    members
}

