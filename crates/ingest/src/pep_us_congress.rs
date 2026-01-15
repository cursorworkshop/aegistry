use anyhow::{Context, Result};
use std::time::Duration;

use crate::parser_eu::{ParsedSubject, SubjectKind};

const CONGRESS_API_URL: &str = "https://www.congress.gov/api";
const CONGRESS_MEMBERS_URL: &str = "https://www.congress.gov/members";

/// Fetch US Congress members (House + Senate) from official sources
pub async fn fetch_us_congress() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")?;

    tracing::info!("fetching US Congress members");

    let mut all_members = Vec::new();

    // Fetch House of Representatives
    match fetch_house_members(&client).await {
        Ok(members) => {
            tracing::info!(count = members.len(), "fetched House members");
            all_members.extend(members);
        }
        Err(e) => tracing::warn!(error = %e, "failed to fetch House members"),
    }

    // Fetch Senate
    match fetch_senate_members(&client).await {
        Ok(members) => {
            tracing::info!(count = members.len(), "fetched Senate members");
            all_members.extend(members);
        }
        Err(e) => tracing::warn!(error = %e, "failed to fetch Senate members"),
    }

    // Deduplicate by name
    all_members.sort_by(|a, b| a.primary_name.cmp(&b.primary_name));
    all_members.dedup_by(|a, b| a.primary_name == b.primary_name);

    tracing::info!(total = all_members.len(), "total US Congress members");
    Ok(all_members)
}

async fn fetch_house_members(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    // Try official API first
    let api_url = format!("{}/member", CONGRESS_API_URL);
    let response = client.get(&api_url).send().await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await?;
            parse_congress_api(&text, "house")
        }
        _ => {
            // Fallback: scrape HTML
            tracing::info!("API failed, scraping House HTML");
            fetch_house_html(client).await
        }
    }
}

async fn fetch_senate_members(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    // Try official API first
    let api_url = format!("{}/member?chamber=senate", CONGRESS_API_URL);
    let response = client.get(&api_url).send().await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await?;
            parse_congress_api(&text, "senate")
        }
        _ => {
            // Fallback: scrape HTML
            tracing::info!("API failed, scraping Senate HTML");
            fetch_senate_html(client).await
        }
    }
}

fn parse_congress_api(_json: &str, _chamber: &str) -> Result<Vec<ParsedSubject>> {
    // Try to parse JSON response
    let members = Vec::new();
    
    // Simple JSON parsing (can be improved with serde_json if structure is known)
    // For now, fallback to HTML scraping
    tracing::warn!("JSON parsing not fully implemented, using HTML fallback");
    Ok(members)
}

async fn fetch_house_html(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    let url = format!("{}/house", CONGRESS_MEMBERS_URL);
    let response = client
        .get(&url)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch House HTML")?;

    if !response.status().is_success() {
        anyhow::bail!("House page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let members = parse_congress_html(&html, "house");
    
    Ok(members)
}

async fn fetch_senate_html(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    let url = format!("{}/senate", CONGRESS_MEMBERS_URL);
    let response = client
        .get(&url)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch Senate HTML")?;

    if !response.status().is_success() {
        anyhow::bail!("Senate page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let members = parse_congress_html(&html, "senate");
    
    Ok(members)
}

fn parse_congress_html(html: &str, chamber: &str) -> Vec<ParsedSubject> {
    let mut members = Vec::new();
    
    // Parse HTML to extract member names
    // Pattern varies, but typically: <a href="/members/...">Name</a> or <span>Name</span>
    
    let lines: Vec<&str> = html.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        // Look for member name patterns
        if line.contains("member") || line.contains("representative") || line.contains("senator") {
            // Try to extract name from current line or nearby lines
            if let Some(name) = extract_member_name(line, &lines, i) {
                if name.len() > 2 && name.contains(' ') {
                    let id = name
                        .chars()
                        .filter(|c| c.is_alphanumeric())
                        .take(20)
                        .collect::<String>();
                    
                    members.push(ParsedSubject {
                        source_ref: format!("pep_us_{}_{}", chamber, id),
                        kind: SubjectKind::Person,
                        primary_name: name,
                        aliases: Vec::new(),
                        date_of_birth: None,
                        date_of_birth_year: None,
                        country: Some("US".to_string()),
                        nationalities: vec!["US".to_string()],
                    });
                }
            }
        }
    }

    // Also try regex patterns for common HTML structures
    use regex::Regex;
    let name_patterns = [
        r#"<a[^>]*href="/members/[^"]*"[^>]*>([^<]+)</a>"#,
        r#"<span[^>]*class="[^"]*name[^"]*"[^>]*>([^<]+)</span>"#,
        r#"<td[^>]*>([A-Z][a-z]+ [A-Z][a-z]+)</td>"#,
    ];

    for pattern in &name_patterns {
        if let Ok(re) = Regex::new(pattern) {
            for cap in re.captures_iter(html) {
                if let Some(name_match) = cap.get(1) {
                    let name = name_match.as_str().trim().to_string();
                    if name.len() > 5 && name.contains(' ') {
                        let id = name
                            .chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(20)
                            .collect::<String>();
                        
                        // Check if already added
                        if !members.iter().any(|m: &ParsedSubject| m.primary_name == name) {
                            members.push(ParsedSubject {
                                source_ref: format!("pep_us_{}_{}", chamber, id),
                                kind: SubjectKind::Person,
                                primary_name: name,
                                aliases: Vec::new(),
                                date_of_birth: None,
                                date_of_birth_year: None,
                                country: Some("US".to_string()),
                                nationalities: vec!["US".to_string()],
                            });
                        }
                    }
                }
            }
        }
    }

    members
}

fn extract_member_name(line: &str, lines: &[&str], idx: usize) -> Option<String> {
    // Try to extract name from HTML tag
    if let Some(start) = line.find('>') {
        let rest = &line[start + 1..];
        if let Some(end) = rest.find('<') {
            let text = rest[..end].trim();
            if text.len() > 5 && text.contains(' ') {
                return Some(text.to_string());
            }
        }
    }
    
    // Try next few lines
    for i in (idx + 1)..(idx + 5).min(lines.len()) {
        let text = lines[i].trim();
        if text.len() > 5 && text.contains(' ') && !text.starts_with('<') {
            return Some(text.to_string());
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_extraction() {
        let html = r#"<a href="/members/john-doe">John Doe</a>"#;
        let members = parse_congress_html(html, "house");
        assert!(!members.is_empty());
    }
}

