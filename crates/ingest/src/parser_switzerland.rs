use anyhow::{Context, Result};
use crate::parser_eu::{ParsedSubject, ParsedAlias, SubjectKind};

pub fn parse_switzerland_sanctions(xml: &[u8]) -> Result<Vec<ParsedSubject>> {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    
    let mut reader = Reader::from_str(std::str::from_utf8(xml)?);
    
    let mut subjects = Vec::new();
    let mut buf = Vec::new();
    let mut current_subject: Option<ParsedSubject> = None;
    let mut current_name = String::new();
    let mut current_country = String::new();
    let mut in_name = false;
    let mut in_country = false;
    
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                match e.name().as_ref() {
                    b"Entity" | b"Person" => {
                        current_subject = Some(ParsedSubject {
                            source_ref: String::new(),
                            kind: if e.name().as_ref() == b"Person" {
                                SubjectKind::Person
                            } else {
                                SubjectKind::Entity
                            },
                            primary_name: String::new(),
                            aliases: Vec::new(),
                            date_of_birth: None,
                            date_of_birth_year: None,
                            country: None,
                            nationalities: Vec::new(),
                        });
                    }
                    b"Name" | b"FullName" => in_name = true,
                    b"Country" | b"Nationality" => in_country = true,
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().unwrap_or_default();
                if in_name {
                    current_name = text.to_string();
                } else if in_country {
                    current_country = text.to_string();
                }
            }
            Ok(Event::End(e)) => {
                match e.name().as_ref() {
                    b"Entity" | b"Person" => {
                        if let Some(mut subject) = current_subject.take() {
                            if !subject.primary_name.is_empty() {
                                subject.source_ref = format!("switzerland_{}", subject.primary_name.replace(' ', "_").to_lowercase());
                                if !current_country.is_empty() {
                                    subject.country = Some(current_country.clone());
                                }
                                subjects.push(subject);
                            }
                            current_name.clear();
                            current_country.clear();
                        }
                    }
                    b"Name" | b"FullName" => {
                        if let Some(ref mut subject) = current_subject {
                            if subject.primary_name.is_empty() {
                                subject.primary_name = current_name.clone();
                            } else {
                                subject.aliases.push(ParsedAlias {
                                    name: current_name.clone(),
                                    alias_type: "aka".to_string(),
                                });
                            }
                        }
                        in_name = false;
                        current_name.clear();
                    }
                    b"Country" | b"Nationality" => {
                        if let Some(ref mut subject) = current_subject {
                            if subject.country.is_none() && !current_country.is_empty() {
                                subject.country = Some(current_country.clone());
                            }
                        }
                        in_country = false;
                        current_country.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!(error = %e, "XML parse error in Switzerland sanctions");
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    
    tracing::info!(count = subjects.len(), "parsed Switzerland sanctions");
    Ok(subjects)
}

