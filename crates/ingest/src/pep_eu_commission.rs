use anyhow::{Context, Result};
use std::time::Duration;

use crate::parser_eu::{ParsedSubject, SubjectKind};

// Try multiple URLs for European Commission
const COMMISSION_URLS: &[&str] = &[
    "https://commissioners.ec.europa.eu/commissioners_en",
    "https://ec.europa.eu/commission/commissioners/",
    "https://commissioners.ec.europa.eu/",
    "https://ec.europa.eu/commission/commissioners/index_en",
];

/// Fetch European Commission members (Commissioners)
pub async fn fetch_eu_commission() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")?;

    // Try each URL until one works
    for url in COMMISSION_URLS {
        tracing::info!(url = url, "trying European Commission URL");
        
        let response = match client
            .get(*url)
            .header("Accept", "text/html")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(url = url, error = %e, "failed to fetch, trying next URL");
                continue;
            }
        };

        if !response.status().is_success() {
            tracing::warn!(url = url, status = %response.status(), "HTTP error, trying next URL");
            continue;
        }

        let html = match response.text().await {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(url = url, error = %e, "failed to read response, trying next URL");
                continue;
            }
        };
        
        let subjects = parse_commission_html(&html);
        
        if !subjects.is_empty() {
            tracing::info!(count = subjects.len(), url = url, "parsed European Commission members");
            return Ok(subjects);
        } else {
            tracing::warn!(url = url, "no subjects parsed, trying next URL");
        }
    }
    
    // If all URLs fail, use fallback list
    tracing::warn!("all Commission URLs failed, using fallback list");
    let subjects = parse_commission_html("");
    Ok(subjects)
}

fn parse_commission_html(html: &str) -> Vec<ParsedSubject> {
    let mut subjects = Vec::new();
    
    // Known commissioners (2024-2029 von der Leyen II Commission)
    // This is a fallback if HTML parsing fails
    let known_commissioners = vec![
        ("Ursula von der Leyen", "DE", "President"),
        ("Maros Sefcovic", "SK", "Executive Vice-President"),
        ("Stephane Sejourne", "FR", "Executive Vice-President"),
        ("Teresa Ribera", "ES", "Executive Vice-President"),
        ("Henna Virkkunen", "FI", "Executive Vice-President"),
        ("Raffaele Fitto", "IT", "Executive Vice-President"),
        ("Kaja Kallas", "EE", "High Representative"),
        ("Valdis Dombrovskis", "LV", "Commissioner"),
        ("Dubravka Suica", "HR", "Commissioner"),
        ("Wopke Hoekstra", "NL", "Commissioner"),
        ("Thierry Breton", "FR", "Commissioner"),
        ("Virginijus Sinkevicius", "LT", "Commissioner"),
        ("Kadri Simson", "EE", "Commissioner"),
        ("Johannes Hahn", "AT", "Commissioner"),
        ("Paolo Gentiloni", "IT", "Commissioner"),
        ("Janusz Wojciechowski", "PL", "Commissioner"),
        ("Stella Kyriakides", "CY", "Commissioner"),
        ("Nicolas Schmit", "LU", "Commissioner"),
        ("Helena Dalli", "MT", "Commissioner"),
        ("Ylva Johansson", "SE", "Commissioner"),
        ("Elisa Ferreira", "PT", "Commissioner"),
        ("Margaritis Schinas", "GR", "Commissioner"),
        ("Didier Reynders", "BE", "Commissioner"),
        ("Adina Valean", "RO", "Commissioner"),
        ("Jutta Urpilainen", "FI", "Commissioner"),
        ("Oliver Varhelyi", "HU", "Commissioner"),
        ("Janez Lenarcic", "SI", "Commissioner"),
    ];

    // First try to parse from HTML
    let parsed = parse_commissioners_from_html(html);
    
    if !parsed.is_empty() {
        subjects = parsed;
    } else {
        // Use known list as fallback
        for (name, country, role) in known_commissioners {
            let id = name.chars()
                .filter(|c| c.is_alphanumeric())
                .take(20)
                .collect::<String>();
            
            subjects.push(ParsedSubject {
                source_ref: format!("pep_eu_comm_{}", id),
                kind: SubjectKind::Person,
                primary_name: name.to_string(),
                aliases: vec![],
                date_of_birth: None,
                date_of_birth_year: None,
                country: Some(country.to_string()),
                nationalities: vec![country.to_string()],
            });
            
            tracing::debug!(name, role, "added commissioner");
        }
    }

    subjects
}

fn parse_commissioners_from_html(html: &str) -> Vec<ParsedSubject> {
    let mut subjects = Vec::new();
    
    // Look for commissioner entries in HTML
    // Common patterns: <h2>Name</h2>, <span class="name">Name</span>, etc.
    
    let mut in_commissioner_section = false;
    
    for line in html.lines() {
        // Detect commissioner section
        if line.contains("commissioner") || line.contains("Commissioner") {
            in_commissioner_section = true;
        }
        
        if in_commissioner_section {
            // Look for name patterns
            if line.contains("<h2") || line.contains("<h3") || line.contains("card-title") {
                if let Some(name) = extract_name_from_html(line) {
                    if is_likely_person_name(&name) {
                        let id = name.chars()
                            .filter(|c| c.is_alphanumeric())
                            .take(20)
                            .collect::<String>();
                        
                        // Try to find country
                        let country = extract_country_nearby(html, &name);
                        
                        subjects.push(ParsedSubject {
                            source_ref: format!("pep_eu_comm_{}", id),
                            kind: SubjectKind::Person,
                            primary_name: name,
                            aliases: vec![],
                            date_of_birth: None,
                            date_of_birth_year: None,
                            country,
                            nationalities: vec![],
                        });
                    }
                }
            }
        }
    }

    // Deduplicate
    subjects.sort_by(|a, b| a.primary_name.cmp(&b.primary_name));
    subjects.dedup_by(|a, b| a.primary_name == b.primary_name);
    
    subjects
}

fn extract_name_from_html(line: &str) -> Option<String> {
    // Strip HTML tags and get text content
    let mut result = String::new();
    let mut in_tag = false;
    
    for c in line.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }
    
    let text = result.trim();
    if text.len() > 3 {
        Some(text.to_string())
    } else {
        None
    }
}

fn is_likely_person_name(text: &str) -> bool {
    // Check if text looks like a person name
    let words: Vec<&str> = text.split_whitespace().collect();
    
    // Should have 2-5 words
    if words.len() < 2 || words.len() > 5 {
        return false;
    }
    
    // First word should start with uppercase
    if let Some(first_char) = words[0].chars().next() {
        if !first_char.is_uppercase() {
            return false;
        }
    }
    
    // Should not contain common non-name words
    let non_name_words = ["the", "and", "of", "for", "to", "in", "on", "at", "by"];
    for word in &words {
        if non_name_words.contains(&word.to_lowercase().as_str()) {
            return false;
        }
    }
    
    true
}

fn extract_country_nearby(html: &str, name: &str) -> Option<String> {
    // Find the position of the name and look for country info nearby
    if let Some(pos) = html.find(name) {
        let context = &html[pos.saturating_sub(500)..std::cmp::min(pos + 500, html.len())];
        
        let countries = [
            ("Germany", "DE"), ("France", "FR"), ("Italy", "IT"), ("Spain", "ES"),
            ("Poland", "PL"), ("Romania", "RO"), ("Netherlands", "NL"), ("Belgium", "BE"),
            ("Greece", "GR"), ("Portugal", "PT"), ("Sweden", "SE"), ("Austria", "AT"),
            ("Bulgaria", "BG"), ("Denmark", "DK"), ("Finland", "FI"), ("Ireland", "IE"),
            ("Croatia", "HR"), ("Slovakia", "SK"), ("Lithuania", "LT"), ("Slovenia", "SI"),
            ("Latvia", "LV"), ("Estonia", "EE"), ("Cyprus", "CY"), ("Luxembourg", "LU"),
            ("Malta", "MT"), ("Czechia", "CZ"), ("Hungary", "HU"),
        ];
        
        for (country_name, code) in countries {
            if context.contains(country_name) {
                return Some(code.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_person_name_check() {
        assert!(is_likely_person_name("Ursula von der Leyen"));
        assert!(is_likely_person_name("Maros Sefcovic"));
        assert!(!is_likely_person_name("the Commission"));
        assert!(!is_likely_person_name("x"));
    }
}

