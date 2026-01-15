use anyhow::{Context, Result};
use std::time::Duration;

use crate::parser_eu::{ParsedSubject, SubjectKind};

const TWEEDE_KAMER_URL: &str = "https://www.tweedekamer.nl";

/// Fetch Dutch Tweede Kamer members from official sources
pub async fn fetch_dutch_tweede_kamer() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")?;

    tracing::info!("fetching Dutch Tweede Kamer members");

    let url = format!("{}/kamerleden_en_commissies/alle_kamerleden", TWEEDE_KAMER_URL);
    let response = client
        .get(&url)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch Tweede Kamer HTML")?;

    if !response.status().is_success() {
        anyhow::bail!("Tweede Kamer page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let members = parse_tweede_kamer_html(&html);
    
    tracing::info!(count = members.len(), "parsed Tweede Kamer members");
    Ok(members)
}

fn parse_tweede_kamer_html(html: &str) -> Vec<ParsedSubject> {
    let mut members = Vec::new();
    
    // Parse HTML to extract member names
    use regex::Regex;
    
    let name_patterns = [
        r#"<a[^>]*href="/kamerleden_en_commissies/alle_kamerleden/[^"]*"[^>]*>([^<]+)</a>"#,
        r#"<span[^>]*class="[^"]*naam[^"]*"[^>]*>([^<]+)</span>"#,
        r#"<h[23][^>]*>([A-Z][a-z]+ [A-Z][a-z]+)</h[23]>"#,
        r#"data-name="([^"]+)"#,
    ];

    for pattern in &name_patterns {
        if let Ok(re) = Regex::new(pattern) {
            for cap in re.captures_iter(html) {
                if let Some(name_match) = cap.get(1) {
                    let name = name_match.as_str().trim().to_string();
                    if name.len() > 5 && name.contains(' ') && !name.contains("Kamerlid") && !name.contains("Tweede Kamer") {
                        let id = name
                            .chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(20)
                            .collect::<String>();
                        
                        // Check if already added
                        if !members.iter().any(|m: &ParsedSubject| m.primary_name == name) {
                            members.push(ParsedSubject {
                                source_ref: format!("pep_nl_tweede_kamer_{}", id),
                                kind: SubjectKind::Person,
                                primary_name: name,
                                aliases: Vec::new(),
                                date_of_birth: None,
                                date_of_birth_year: None,
                                country: Some("NL".to_string()),
                                nationalities: vec!["NL".to_string()],
                            });
                        }
                    }
                }
            }
        }
    }
    
    members
}

