use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

use crate::parser_eu::{ParsedAlias, ParsedSubject, SubjectKind};

const MEP_API_URL: &str = "https://www.europarl.europa.eu/meps/en/full-list/all";
const MEP_XML_URL: &str = "https://www.europarl.europa.eu/meps/en/xml/";

#[derive(Debug, Deserialize)]
struct MepXmlResponse {
    #[serde(rename = "mep", default)]
    meps: Vec<MepEntry>,
}

#[derive(Debug, Deserialize)]
struct MepEntry {
    #[serde(rename = "id")]
    id: Option<String>,
    #[serde(rename = "fullName")]
    full_name: Option<String>,
    #[serde(rename = "country")]
    country: Option<String>,
    #[serde(rename = "politicalGroup")]
    political_group: Option<String>,
    #[serde(rename = "nationalPoliticalGroup")]
    national_party: Option<String>,
}

/// Fetch EU Parliament MEPs from the official API
pub async fn fetch_eu_parliament_meps() -> Result<Vec<ParsedSubject>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")?;

    tracing::info!(url = MEP_XML_URL, "fetching EU Parliament MEPs");

    // Try XML endpoint first
    let response = client
        .get(MEP_XML_URL)
        .header("Accept", "application/xml")
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await?;
            parse_mep_xml(&text)
        }
        _ => {
            // Fallback: scrape the HTML page
            tracing::info!("XML endpoint failed, trying HTML scrape");
            fetch_meps_from_html(&client).await
        }
    }
}

fn parse_mep_xml(xml: &str) -> Result<Vec<ParsedSubject>> {
    // Simple XML parsing for MEP data
    let mut subjects = Vec::new();
    
    // Parse XML manually since structure varies
    for line in xml.lines() {
        if line.contains("<mep") || line.contains("<member") {
            // Extract attributes
            if let Some(subject) = parse_mep_line(line) {
                subjects.push(subject);
            }
        }
    }

    if subjects.is_empty() {
        // Try alternative parsing
        subjects = parse_mep_xml_structured(xml)?;
    }

    tracing::info!(count = subjects.len(), "parsed EU Parliament MEPs");
    Ok(subjects)
}

fn parse_mep_line(line: &str) -> Option<ParsedSubject> {
    // Extract name from various XML formats
    let name = extract_attr(line, "fullName")
        .or_else(|| extract_attr(line, "name"))
        .or_else(|| extract_content(line))?;

    if name.is_empty() {
        return None;
    }

    let id = extract_attr(line, "id")
        .unwrap_or_else(|| name.chars().filter(|c| c.is_alphanumeric()).take(20).collect());

    let country = extract_attr(line, "country")
        .and_then(|c| country_name_to_iso(&c));

    Some(ParsedSubject {
        source_ref: format!("pep_eu_mep_{}", id),
        kind: SubjectKind::Person,
        primary_name: name,
        aliases: Vec::new(),
        date_of_birth: None,
        date_of_birth_year: None,
        country,
        nationalities: Vec::new(),
    })
}

fn parse_mep_xml_structured(xml: &str) -> Result<Vec<ParsedSubject>> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut subjects = Vec::new();
    let mut buf = Vec::new();
    let mut in_mep = false;
    let mut current_name = String::new();
    let mut current_id = String::new();
    let mut current_country = String::new();
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_element = tag.clone();
                
                if tag == "mep" || tag == "member" || tag == "MEP" {
                    in_mep = true;
                    current_name.clear();
                    current_id.clear();
                    current_country.clear();
                    
                    // Check for attributes
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        let value = String::from_utf8_lossy(&attr.value).to_string();
                        match key.as_str() {
                            "id" => current_id = value,
                            "fullName" | "name" => current_name = value,
                            "country" => current_country = value,
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if (tag == "mep" || tag == "member" || tag == "MEP") && in_mep {
                    if !current_name.is_empty() {
                        let id = if current_id.is_empty() {
                            current_name.chars().filter(|c| c.is_alphanumeric()).take(20).collect()
                        } else {
                            current_id.clone()
                        };
                        
                        subjects.push(ParsedSubject {
                            source_ref: format!("pep_eu_mep_{}", id),
                            kind: SubjectKind::Person,
                            primary_name: current_name.clone(),
                            aliases: Vec::new(),
                            date_of_birth: None,
                            date_of_birth_year: None,
                            country: country_name_to_iso(&current_country),
                            nationalities: Vec::new(),
                        });
                    }
                    in_mep = false;
                }
                current_element.clear();
            }
            Ok(Event::Text(ref e)) => {
                if in_mep {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        match current_element.as_str() {
                            "fullName" | "name" | "Name" => current_name = text,
                            "id" | "Id" | "ID" => current_id = text,
                            "country" | "Country" => current_country = text,
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(subjects)
}

async fn fetch_meps_from_html(client: &reqwest::Client) -> Result<Vec<ParsedSubject>> {
    // Fetch the HTML page and extract MEP names
    let response = client
        .get(MEP_API_URL)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch MEP HTML page")?;

    if !response.status().is_success() {
        anyhow::bail!("MEP page returned HTTP {}", response.status());
    }

    let html = response.text().await?;
    let subjects = parse_mep_html(&html);
    
    tracing::info!(count = subjects.len(), "parsed MEPs from HTML");
    Ok(subjects)
}

fn parse_mep_html(html: &str) -> Vec<ParsedSubject> {
    let mut subjects = Vec::new();
    
    // Look for MEP entries in HTML
    // Pattern: <a class="erpl_member-list-item-content" ... data-mep-id="..." ...>name</a>
    // Or: <span class="t-item">Name</span>
    
    for line in html.lines() {
        // Try to find MEP entries
        if line.contains("erpl_member") || line.contains("mep-item") || line.contains("t-item") {
            if let Some(name) = extract_html_text(line) {
                if name.len() > 2 && name.contains(' ') {
                    let id = name.chars().filter(|c| c.is_alphanumeric()).take(20).collect::<String>();
                    
                    // Try to extract country from nearby content
                    let country = extract_country_from_context(line);
                    
                    subjects.push(ParsedSubject {
                        source_ref: format!("pep_eu_mep_{}", id),
                        kind: SubjectKind::Person,
                        primary_name: name,
                        aliases: Vec::new(),
                        date_of_birth: None,
                        date_of_birth_year: None,
                        country,
                        nationalities: Vec::new(),
                    });
                }
            }
        }
    }

    // Deduplicate by name
    subjects.sort_by(|a, b| a.primary_name.cmp(&b.primary_name));
    subjects.dedup_by(|a, b| a.primary_name == b.primary_name);
    
    subjects
}

fn extract_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    if let Some(start) = line.find(&pattern) {
        let rest = &line[start + pattern.len()..];
        if let Some(end) = rest.find('"') {
            let value = &rest[..end];
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_content(line: &str) -> Option<String> {
    // Extract text between > and <
    if let Some(start) = line.find('>') {
        let rest = &line[start + 1..];
        if let Some(end) = rest.find('<') {
            let content = rest[..end].trim();
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
    }
    None
}

fn extract_html_text(line: &str) -> Option<String> {
    // Extract text from HTML tags
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
    
    let text = result.trim().to_string();
    if text.len() > 2 {
        Some(text)
    } else {
        None
    }
}

fn extract_country_from_context(line: &str) -> Option<String> {
    // Common EU country codes that might appear in the HTML
    let countries = [
        ("Germany", "DE"), ("France", "FR"), ("Italy", "IT"), ("Spain", "ES"),
        ("Poland", "PL"), ("Romania", "RO"), ("Netherlands", "NL"), ("Belgium", "BE"),
        ("Greece", "GR"), ("Portugal", "PT"), ("Sweden", "SE"), ("Austria", "AT"),
        ("Bulgaria", "BG"), ("Denmark", "DK"), ("Finland", "FI"), ("Ireland", "IE"),
        ("Croatia", "HR"), ("Slovakia", "SK"), ("Lithuania", "LT"), ("Slovenia", "SI"),
        ("Latvia", "LV"), ("Estonia", "EE"), ("Cyprus", "CY"), ("Luxembourg", "LU"),
        ("Malta", "MT"), ("Czechia", "CZ"), ("Czech Republic", "CZ"), ("Hungary", "HU"),
    ];
    
    for (name, code) in countries {
        if line.contains(name) {
            return Some(code.to_string());
        }
    }
    None
}

fn country_name_to_iso(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    
    if name.len() == 2 {
        return Some(name.to_uppercase());
    }
    
    let lower = name.to_lowercase();
    let code = match lower.as_str() {
        "germany" | "deutschland" => "DE",
        "france" => "FR",
        "italy" | "italia" => "IT",
        "spain" | "espana" => "ES",
        "poland" | "polska" => "PL",
        "romania" => "RO",
        "netherlands" | "nederland" => "NL",
        "belgium" | "belgique" | "belgie" => "BE",
        "greece" | "hellas" => "GR",
        "portugal" => "PT",
        "sweden" | "sverige" => "SE",
        "austria" | "osterreich" => "AT",
        "bulgaria" => "BG",
        "denmark" | "danmark" => "DK",
        "finland" | "suomi" => "FI",
        "ireland" | "eire" => "IE",
        "croatia" | "hrvatska" => "HR",
        "slovakia" | "slovensko" => "SK",
        "lithuania" | "lietuva" => "LT",
        "slovenia" | "slovenija" => "SI",
        "latvia" | "latvija" => "LV",
        "estonia" | "eesti" => "EE",
        "cyprus" => "CY",
        "luxembourg" => "LU",
        "malta" => "MT",
        "czechia" | "czech republic" | "cesko" => "CZ",
        "hungary" | "magyarorszag" => "HU",
        _ => return None,
    };
    
    Some(code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_mapping() {
        assert_eq!(country_name_to_iso("Germany"), Some("DE".to_string()));
        assert_eq!(country_name_to_iso("DE"), Some("DE".to_string()));
        assert_eq!(country_name_to_iso("unknown"), None);
    }
}



