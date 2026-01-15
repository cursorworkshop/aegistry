use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::parser_eu::{ParsedAlias, ParsedSubject, SubjectKind};

/// Parse UN Security Council consolidated sanctions list XML
pub fn parse_un_xml(xml_data: &[u8]) -> Result<Vec<ParsedSubject>> {
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(true);

    let mut subjects = Vec::new();
    let mut buf = Vec::new();
    
    // Track current element context
    let mut in_individual = false;
    let mut in_entity = false;
    let mut current_subject: Option<SubjectBuilder> = None;
    let mut current_element = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_element = name.clone();
                
                match name.as_str() {
                    "INDIVIDUAL" => {
                        in_individual = true;
                        current_subject = Some(SubjectBuilder::new(SubjectKind::Person));
                    }
                    "ENTITY" => {
                        in_entity = true;
                        current_subject = Some(SubjectBuilder::new(SubjectKind::Entity));
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                
                match name.as_str() {
                    "INDIVIDUAL" | "ENTITY" => {
                        if let Some(builder) = current_subject.take() {
                            if let Some(subject) = builder.build("un") {
                                subjects.push(subject);
                            }
                        }
                        in_individual = false;
                        in_entity = false;
                    }
                    _ => {}
                }
                current_element.clear();
            }
            Ok(Event::Text(ref e)) => {
                if (in_individual || in_entity) && current_subject.is_some() {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if text.is_empty() {
                        continue;
                    }
                    
                    let builder = current_subject.as_mut().unwrap();
                    
                    match current_element.as_str() {
                        "DATAID" => {
                            builder.source_ref = Some(text);
                        }
                        "FIRST_NAME" | "SECOND_NAME" | "THIRD_NAME" | "FOURTH_NAME" => {
                            builder.add_name_part(&text);
                        }
                        "NAME_ORIGINAL_SCRIPT" => {
                            builder.add_alias(&text);
                        }
                        "ALIAS_NAME" => {
                            builder.add_alias(&text);
                        }
                        "NATIONALITY" => {
                            if text.len() == 2 {
                                builder.add_nationality(&text.to_uppercase());
                            }
                        }
                        "DATE_OF_BIRTH" => {
                            builder.date_of_birth = Some(text.clone());
                            if let Some(year_str) = text.split('-').next() {
                                if let Ok(year) = year_str.parse::<i32>() {
                                    if year > 1900 && year < 2100 {
                                        builder.date_of_birth_year = Some(year);
                                    }
                                }
                            }
                        }
                        "YEAR_OF_BIRTH" => {
                            if let Ok(year) = text.parse::<i32>() {
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

    tracing::info!(count = subjects.len(), "parsed UN sanctions subjects");
    Ok(subjects)
}

struct SubjectBuilder {
    source_ref: Option<String>,
    kind: SubjectKind,
    name_parts: Vec<String>,
    aliases: Vec<String>,
    date_of_birth: Option<String>,
    date_of_birth_year: Option<i32>,
    country: Option<String>,
    nationalities: Vec<String>,
}

impl SubjectBuilder {
    fn new(kind: SubjectKind) -> Self {
        Self {
            source_ref: None,
            kind,
            name_parts: Vec::new(),
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

    fn add_nationality(&mut self, nat: &str) {
        if !nat.is_empty() {
            if self.country.is_none() {
                self.country = Some(nat.to_string());
            }
            self.nationalities.push(nat.to_string());
        }
    }

    fn build(self, prefix: &str) -> Option<ParsedSubject> {
        let primary_name = self.name_parts.join(" ");
        if primary_name.is_empty() {
            return None;
        }

        let source_ref = match self.source_ref {
            Some(ref id) => format!("{}_{}", prefix, id),
            None => format!("{}_{}", prefix, primary_name.chars().filter(|c| c.is_alphanumeric()).take(20).collect::<String>()),
        };

        let aliases = self.aliases
            .into_iter()
            .map(|name| ParsedAlias { name, alias_type: "aka".to_string() })
            .collect();

        Some(ParsedSubject {
            source_ref,
            kind: self.kind,
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
    fn parse_un_individual() {
        let xml = r#"<?xml version="1.0"?>
        <CONSOLIDATED_LIST>
            <INDIVIDUALS>
                <INDIVIDUAL>
                    <DATAID>12345</DATAID>
                    <FIRST_NAME>John</FIRST_NAME>
                    <SECOND_NAME>Doe</SECOND_NAME>
                    <NATIONALITY>US</NATIONALITY>
                    <DATE_OF_BIRTH>1970-01-15</DATE_OF_BIRTH>
                </INDIVIDUAL>
            </INDIVIDUALS>
        </CONSOLIDATED_LIST>"#;
        
        let subjects = parse_un_xml(xml.as_bytes()).unwrap();
        assert_eq!(subjects.len(), 1);
        assert_eq!(subjects[0].source_ref, "un_12345");
        assert_eq!(subjects[0].primary_name, "John Doe");
        assert_eq!(subjects[0].country, Some("US".to_string()));
        assert_eq!(subjects[0].date_of_birth_year, Some(1970));
    }
}
