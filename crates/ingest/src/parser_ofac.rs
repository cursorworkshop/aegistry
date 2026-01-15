use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::parser_eu::{ParsedAlias, ParsedSubject, SubjectKind};

/// Parse OFAC SDN list XML
pub fn parse_ofac_xml(xml_data: &[u8]) -> Result<Vec<ParsedSubject>> {
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(true);

    let mut subjects = Vec::new();
    let mut buf = Vec::new();
    
    // Track current element context
    let mut in_sdn_entry = false;
    let mut current_builder: Option<SubjectBuilder> = None;
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_element = name.clone();
                
                if name == "sdnEntry" {
                    in_sdn_entry = true;
                    current_builder = Some(SubjectBuilder::new());
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                
                if name == "sdnEntry" {
                    if let Some(builder) = current_builder.take() {
                        if let Some(subject) = builder.build() {
                            subjects.push(subject);
                        }
                    }
                    in_sdn_entry = false;
                }
                current_element.clear();
            }
            Ok(Event::Text(ref e)) => {
                if in_sdn_entry && current_builder.is_some() {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    
                    let builder = current_builder.as_mut().unwrap();
                    
                    match current_element.as_str() {
                        "uid" => {
                            builder.source_ref = Some(text);
                        }
                        "sdnType" => {
                            builder.sdn_type = Some(text);
                        }
                        "firstName" => {
                            builder.first_name = Some(text);
                        }
                        "lastName" => {
                            builder.last_name = Some(text);
                        }
                        "aka" => {
                            builder.add_alias(&text);
                        }
                        "country" => {
                            // OFAC uses full country names
                            builder.add_country(&text);
                        }
                        "dateOfBirth" => {
                            builder.date_of_birth = Some(text.clone());
                            if let Some(year) = extract_year(&text) {
                                builder.date_of_birth_year = Some(year);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!(error = %e, "XML parse error, continuing");
            }
            _ => {}
        }
        buf.clear();
    }

    tracing::info!(count = subjects.len(), "parsed OFAC SDN subjects");
    Ok(subjects)
}

fn extract_year(date_str: &str) -> Option<i32> {
    // Try various date formats
    // YYYY-MM-DD, DD MMM YYYY, YYYY, etc.
    
    // Try 4-digit year at start
    if date_str.len() >= 4 {
        if let Ok(year) = date_str[..4].parse::<i32>() {
            if year > 1900 && year < 2100 {
                return Some(year);
            }
        }
    }
    
    // Try to find 4 consecutive digits
    for i in 0..date_str.len().saturating_sub(3) {
        if let Ok(year) = date_str[i..i+4].parse::<i32>() {
            if year > 1900 && year < 2100 {
                return Some(year);
            }
        }
    }
    
    None
}

struct SubjectBuilder {
    source_ref: Option<String>,
    sdn_type: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    aliases: Vec<String>,
    date_of_birth: Option<String>,
    date_of_birth_year: Option<i32>,
    country: Option<String>,
    nationalities: Vec<String>,
}

impl SubjectBuilder {
    fn new() -> Self {
        Self {
            source_ref: None,
            sdn_type: None,
            first_name: None,
            last_name: None,
            aliases: Vec::new(),
            date_of_birth: None,
            date_of_birth_year: None,
            country: None,
            nationalities: Vec::new(),
        }
    }

    fn add_alias(&mut self, alias: &str) {
        if !alias.is_empty() {
            self.aliases.push(alias.to_string());
        }
    }

    fn add_country(&mut self, country: &str) {
        if !country.is_empty() && self.country.is_none() {
            // Try to extract ISO code from country name
            let iso = country_to_iso(country);
            self.country = Some(iso.clone());
            self.nationalities.push(iso);
        }
    }

    fn build(self) -> Option<ParsedSubject> {
        // Build primary name from first + last
        let primary_name = match (&self.first_name, &self.last_name) {
            (Some(first), Some(last)) => format!("{} {}", first, last),
            (Some(first), None) => first.clone(),
            (None, Some(last)) => last.clone(),
            (None, None) => return None,
        };

        if primary_name.is_empty() {
            return None;
        }

        let source_ref = match self.source_ref {
            Some(ref id) => format!("ofac_{}", id),
            None => format!("ofac_{}", primary_name.chars().filter(|c| c.is_alphanumeric()).take(20).collect::<String>()),
        };

        let kind = match self.sdn_type.as_deref() {
            Some(t) if t.to_uppercase() == "INDIVIDUAL" => SubjectKind::Person,
            _ => SubjectKind::Entity,
        };

        let aliases = self.aliases
            .into_iter()
            .map(|name| ParsedAlias { name, alias_type: "aka".to_string() })
            .collect();

        Some(ParsedSubject {
            source_ref,
            kind,
            primary_name,
            aliases,
            date_of_birth: self.date_of_birth,
            date_of_birth_year: self.date_of_birth_year,
            country: self.country,
            nationalities: self.nationalities,
        })
    }
}

fn country_to_iso(country: &str) -> String {
    // Simple mapping of common country names to ISO codes
    let lower = country.to_lowercase();
    let code = match lower.as_str() {
        "russia" | "russian federation" => "RU",
        "china" | "people's republic of china" => "CN",
        "iran" | "islamic republic of iran" => "IR",
        "north korea" | "democratic people's republic of korea" => "KP",
        "syria" | "syrian arab republic" => "SY",
        "cuba" => "CU",
        "venezuela" => "VE",
        "belarus" => "BY",
        "myanmar" | "burma" => "MM",
        "afghanistan" => "AF",
        "iraq" => "IQ",
        "libya" => "LY",
        "sudan" => "SD",
        "yemen" => "YE",
        "united states" | "usa" => "US",
        "united kingdom" | "uk" => "GB",
        _ => {
            // Return first 2 uppercase chars as fallback
            return country.chars()
                .filter(|c| c.is_ascii_uppercase())
                .take(2)
                .collect::<String>()
                .to_uppercase();
        }
    };
    code.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ofac_individual() {
        let xml = r#"<?xml version="1.0"?>
        <sdnList>
            <sdnEntry>
                <uid>12345</uid>
                <sdnType>Individual</sdnType>
                <firstName>John</firstName>
                <lastName>Doe</lastName>
                <dateOfBirth>1970-01-15</dateOfBirth>
            </sdnEntry>
        </sdnList>"#;
        
        let subjects = parse_ofac_xml(xml.as_bytes()).unwrap();
        assert_eq!(subjects.len(), 1);
        assert_eq!(subjects[0].source_ref, "ofac_12345");
        assert!(subjects[0].primary_name.contains("John"));
        assert!(subjects[0].primary_name.contains("Doe"));
        assert_eq!(subjects[0].date_of_birth_year, Some(1970));
    }

    #[test]
    fn extract_year_various_formats() {
        assert_eq!(extract_year("1970-01-15"), Some(1970));
        assert_eq!(extract_year("15 Jan 1985"), Some(1985));
        assert_eq!(extract_year("circa 1960"), Some(1960));
        assert_eq!(extract_year("unknown"), None);
    }
}
