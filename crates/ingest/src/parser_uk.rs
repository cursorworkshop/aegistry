use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::parser_eu::{ParsedAlias, ParsedSubject, SubjectKind};

/// Parse UK Sanctions List XML
pub fn parse_uk_xml(xml_data: &[u8]) -> Result<Vec<ParsedSubject>> {
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(true);

    let mut subjects = Vec::new();
    let mut buf = Vec::new();
    
    // Track current element context
    let mut in_designation = false;
    let mut current_builder: Option<SubjectBuilder> = None;
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_element = name.clone();
                
                // UK uses "Designation" as the entry element
                if name == "Designation" {
                    in_designation = true;
                    current_builder = Some(SubjectBuilder::new());
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                
                if name == "Designation" {
                    if let Some(builder) = current_builder.take() {
                        if let Some(subject) = builder.build() {
                            subjects.push(subject);
                        }
                    }
                    in_designation = false;
                }
                current_element.clear();
            }
            Ok(Event::Text(ref e)) => {
                if in_designation && current_builder.is_some() {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    
                    let builder = current_builder.as_mut().unwrap();
                    
                    match current_element.as_str() {
                        "UniqueID" | "OFSIGroupID" => {
                            if builder.source_ref.is_none() {
                                builder.source_ref = Some(text);
                            }
                        }
                        "GroupTypeDescription" => {
                            builder.group_type = Some(text);
                        }
                        "Name1" | "Name2" | "Name3" | "Name4" | "Name5" | "Name6" => {
                            builder.add_name_part(&text);
                        }
                        "FullName" | "WholeName" => {
                            builder.full_name = Some(text);
                        }
                        "AliasName" | "Alias" => {
                            builder.add_alias(&text);
                        }
                        "Country" | "Nationality" => {
                            builder.add_country(&text);
                        }
                        "DOB" | "DateOfBirth" => {
                            builder.date_of_birth = Some(text.clone());
                            if let Some(year) = extract_year_uk(&text) {
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

    tracing::info!(count = subjects.len(), "parsed UK sanctions subjects");
    Ok(subjects)
}

fn extract_year_uk(date_str: &str) -> Option<i32> {
    // UK format: DD/MM/YYYY or YYYY-MM-DD or various text formats
    
    // Try YYYY-MM-DD
    if date_str.len() >= 4 {
        if let Ok(year) = date_str[..4].parse::<i32>() {
            if year > 1900 && year < 2100 {
                return Some(year);
            }
        }
    }
    
    // Try DD/MM/YYYY
    if date_str.contains('/') {
        let parts: Vec<&str> = date_str.split('/').collect();
        if parts.len() == 3 {
            if let Ok(year) = parts[2].parse::<i32>() {
                if year > 1900 && year < 2100 {
                    return Some(year);
                }
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
    group_type: Option<String>,
    name_parts: Vec<String>,
    full_name: Option<String>,
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
            group_type: None,
            name_parts: Vec::new(),
            full_name: None,
            aliases: Vec::new(),
            date_of_birth: None,
            date_of_birth_year: None,
            country: None,
            nationalities: Vec::new(),
        }
    }

    fn add_name_part(&mut self, part: &str) {
        if !part.is_empty() {
            self.name_parts.push(part.to_string());
        }
    }

    fn add_alias(&mut self, alias: &str) {
        if !alias.is_empty() {
            self.aliases.push(alias.to_string());
        }
    }

    fn add_country(&mut self, country: &str) {
        if !country.is_empty() && self.country.is_none() {
            // UK uses ISO codes or full names
            let iso = if country.len() == 2 {
                country.to_uppercase()
            } else {
                // Try to extract ISO code from parentheses or use first 2 uppercase chars
                country.chars()
                    .filter(|c| c.is_ascii_uppercase())
                    .take(2)
                    .collect::<String>()
            };
            if iso.len() == 2 {
                self.country = Some(iso.clone());
                self.nationalities.push(iso);
            }
        }
    }

    fn build(self) -> Option<ParsedSubject> {
        // Use full_name if available, otherwise join name parts
        let primary_name = self.full_name.unwrap_or_else(|| self.name_parts.join(" "));
        
        if primary_name.is_empty() {
            return None;
        }

        let source_ref = match self.source_ref {
            Some(ref id) => format!("uk_{}", id),
            None => format!("uk_{}", primary_name.chars().filter(|c| c.is_alphanumeric()).take(20).collect::<String>()),
        };

        let kind = match self.group_type.as_deref() {
            Some(t) if t.to_uppercase().contains("INDIVIDUAL") || t.to_uppercase().contains("PERSON") => SubjectKind::Person,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_uk_individual() {
        let xml = r#"<?xml version="1.0"?>
        <Designations>
            <Designation>
                <UniqueID>12345</UniqueID>
                <GroupTypeDescription>Individual</GroupTypeDescription>
                <Name1>John</Name1>
                <Name6>Doe</Name6>
                <DOB>15/01/1970</DOB>
                <Nationality>GB</Nationality>
            </Designation>
        </Designations>"#;
        
        let subjects = parse_uk_xml(xml.as_bytes()).unwrap();
        assert_eq!(subjects.len(), 1);
        assert_eq!(subjects[0].source_ref, "uk_12345");
        assert!(subjects[0].primary_name.contains("John"));
        assert!(subjects[0].primary_name.contains("Doe"));
        assert_eq!(subjects[0].date_of_birth_year, Some(1970));
        assert_eq!(subjects[0].country, Some("GB".to_string()));
    }

    #[test]
    fn extract_year_uk_formats() {
        assert_eq!(extract_year_uk("15/01/1970"), Some(1970));
        assert_eq!(extract_year_uk("1985-01-15"), Some(1985));
        assert_eq!(extract_year_uk("circa 1960"), Some(1960));
    }
}
