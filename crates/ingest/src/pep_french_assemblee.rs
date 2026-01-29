use anyhow::{Context, Result};
use std::time::Duration;

use crate::parser_eu::{ParsedSubject, SubjectKind};

const ASSEMBLEE_URL: &str = "https://www.assemblee-nationale.fr";

/// Fetch French Assemblée Nationale members from official sources
pub async fn fetch_french_assemblee() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")?;

    tracing::info!("fetching French Assemblée Nationale members");

    let url = format!("{}/deputes/liste/alphabetique", ASSEMBLEE_URL);
    let response = client
        .get(&url)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch Assemblée HTML")?;

    if !response.status().is_success() {
        anyhow::bail!("Assemblée page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let members = parse_assemblee_html(&html);
    
    tracing::info!(count = members.len(), "parsed Assemblée Nationale members");
    Ok(members)
}

fn parse_assemblee_html(html: &str) -> Vec<ParsedSubject> {
    let mut members = Vec::new();
    
    // Parse HTML to extract member names
    use regex::Regex;
    
    let name_patterns = [
        r#"<a[^>]*href="/deputes/[^"]*"[^>]*>([^<]+)</a>"#,
        r#"<span[^>]*class="[^"]*nom[^"]*"[^>]*>([^<]+)</span>"#,
        r#"<td[^>]*class="[^"]*nom[^"]*"[^>]*>([^<]+)</td>"#,
        r#"<h[23][^>]*>([A-ZÉÈÊËÀÂ][a-zéèêëàâ]+ [A-ZÉÈÊËÀÂ][a-zéèêëàâ]+)</h[23]>"#,
    ];

    for pattern in &name_patterns {
        if let Ok(re) = Regex::new(pattern) {
            for cap in re.captures_iter(html) {
                if let Some(name_match) = cap.get(1) {
                    let name = name_match.as_str().trim().to_string();
                    if name.len() > 5 && name.contains(' ') && !name.contains("Député") && !name.contains("M.") && !name.contains("Mme") {
                        let id = name
                            .chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(20)
                            .collect::<String>();
                        
                        // Check if already added
                        if !members.iter().any(|m: &ParsedSubject| m.primary_name == name) {
                            members.push(ParsedSubject {
                                source_ref: format!("pep_fr_assemblee_{}", id),
                                kind: SubjectKind::Person,
                                primary_name: name,
                                aliases: Vec::new(),
                                date_of_birth: None,
                                date_of_birth_year: None,
                                country: Some("FR".to_string()),
                                nationalities: vec!["FR".to_string()],
                            });
                        }
                    }
                }
            }
        }
    }
    
    members
}

