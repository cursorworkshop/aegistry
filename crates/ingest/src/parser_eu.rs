use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedSubject {
    pub source_ref: String,
    pub kind: SubjectKind,
    pub primary_name: String,
    pub aliases: Vec<ParsedAlias>,
    pub date_of_birth: Option<String>,
    pub date_of_birth_year: Option<i32>,
    pub country: Option<String>,
    pub nationalities: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubjectKind {
    Person,
    Entity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedAlias {
    pub name: String,
    pub alias_type: String,
}

pub fn parse_eu_xml(xml_data: &[u8]) -> Result<Vec<ParsedSubject>> {
    let mut reader = Reader::from_reader(xml_data);
    reader.config_mut().trim_text(true);

    let mut subjects = Vec::new();
    let mut buf = Vec::new();

    let mut in_sanction_entity = false;
    let mut current_builder: Option<SubjectBuilder> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                
                match tag_name.as_str() {
                    "sanctionEntity" => {
                        in_sanction_entity = true;
                        let mut builder = SubjectBuilder::default();
                        
                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "logicalId" => builder.source_ref = Some(value),
                                "euReferenceNumber" => {
                                    if builder.source_ref.is_none() {
                                        builder.source_ref = Some(value);
                                    }
                                }
                                _ => {}
                            }
                        }
                        current_builder = Some(builder);
                    }
                    "subjectType" if in_sanction_entity => {
                        if let Some(ref mut builder) = current_builder {
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                                let value = String::from_utf8_lossy(&attr.value).to_string();
                                if key == "code" || key == "classificationCode" {
                                    let lower = value.to_lowercase();
                                    if lower == "person" || lower == "p" {
                                        builder.kind = Some(SubjectKind::Person);
                                    } else if lower == "enterprise" || lower == "e" {
                                        builder.kind = Some(SubjectKind::Entity);
                                    }
                                }
                            }
                        }
                    }
                    "nameAlias" if in_sanction_entity => {
                        if let Some(ref mut builder) = current_builder {
                            let mut whole_name = String::new();
                            let mut first_name = String::new();
                            let mut last_name = String::new();
                            
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                                let value = String::from_utf8_lossy(&attr.value).to_string();
                                match key.as_str() {
                                    "wholeName" => whole_name = value,
                                    "firstName" => first_name = value,
                                    "lastName" => last_name = value,
                                    _ => {}
                                }
                            }
                            
                            let name = if !whole_name.is_empty() {
                                whole_name
                            } else if !first_name.is_empty() || !last_name.is_empty() {
                                format!("{} {}", first_name, last_name).trim().to_string()
                            } else {
                                String::new()
                            };
                            
                            if !name.is_empty() {
                                if builder.primary_name.is_none() {
                                    builder.primary_name = Some(name);
                                } else {
                                    builder.aliases.push(ParsedAlias {
                                        name,
                                        alias_type: "aka".to_string(),
                                    });
                                }
                            }
                        }
                    }
                    "citizenship" if in_sanction_entity => {
                        if let Some(ref mut builder) = current_builder {
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                                let value = String::from_utf8_lossy(&attr.value).to_string();
                                if key == "countryIso2Code" && !value.is_empty() && value != "00" {
                                    builder.nationalities.push(value.to_uppercase());
                                }
                            }
                        }
                    }
                    "birthdate" if in_sanction_entity => {
                        if let Some(ref mut builder) = current_builder {
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                                let value = String::from_utf8_lossy(&attr.value).to_string();
                                match key.as_str() {
                                    "year" => {
                                        if let Ok(y) = value.parse::<i32>() {
                                            if builder.date_of_birth_year.is_none() {
                                                builder.date_of_birth_year = Some(y);
                                            }
                                        }
                                    }
                                    "birthdate" => {
                                        if !value.is_empty() && builder.date_of_birth.is_none() {
                                            builder.date_of_birth = Some(value.clone());
                                            if builder.date_of_birth_year.is_none() {
                                                if let Some(y) = extract_year(&value) {
                                                    builder.date_of_birth_year = Some(y);
                                                }
                                            }
                                        }
                                    }
                                    "countryIso2Code" => {
                                        if builder.country.is_none() && !value.is_empty() && value != "00" {
                                            builder.country = Some(value.to_uppercase());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag_name == "sanctionEntity" {
                    in_sanction_entity = false;
                    if let Some(builder) = current_builder.take() {
                        if let Some(subject) = builder.build() {
                            subjects.push(subject);
                        }
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

    tracing::info!(count = subjects.len(), "parsed EU sanctions subjects");
    Ok(subjects)
}

fn extract_year(date_str: &str) -> Option<i32> {
    // Try to parse YYYY-MM-DD or just YYYY
    if date_str.len() >= 4 {
        date_str[..4].parse().ok()
    } else {
        None
    }
}

#[derive(Default)]
struct SubjectBuilder {
    source_ref: Option<String>,
    kind: Option<SubjectKind>,
    primary_name: Option<String>,
    aliases: Vec<ParsedAlias>,
    date_of_birth: Option<String>,
    date_of_birth_year: Option<i32>,
    country: Option<String>,
    nationalities: Vec<String>,
}

impl SubjectBuilder {
    fn build(self) -> Option<ParsedSubject> {
        let primary_name = self.primary_name?;
        if primary_name.is_empty() {
            return None;
        }

        let source_ref = self.source_ref.unwrap_or_else(|| {
            format!("eu_{}", primary_name.chars().filter(|c| c.is_alphanumeric()).take(20).collect::<String>())
        });

        // Use first nationality as country if no country set
        let country = self.country.or_else(|| self.nationalities.first().cloned());

        Some(ParsedSubject {
            source_ref,
            kind: self.kind.unwrap_or(SubjectKind::Person),
            primary_name,
            aliases: self.aliases,
            date_of_birth: self.date_of_birth,
            date_of_birth_year: self.date_of_birth_year,
            country,
            nationalities: self.nationalities,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_real_eu_format() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<export xmlns="http://eu.europa.ec/fpi/fsd/export" generationDate="2025-11-21T17:00:34.049+01:00">
    <sanctionEntity designationDetails="" euReferenceNumber="EU.27.28" logicalId="13">
        <subjectType code="person" classificationCode="P"/>
        <nameAlias firstName="Saddam" lastName="Hussein Al-Tikriti" wholeName="Saddam Hussein Al-Tikriti" logicalId="17"/>
        <nameAlias wholeName="Abu Ali" logicalId="19"/>
        <citizenship countryIso2Code="IQ" countryDescription="IRAQ"/>
        <birthdate year="1937" birthdate="1937-04-28" countryIso2Code="IQ"/>
    </sanctionEntity>
</export>"#;

        let subjects = parse_eu_xml(xml.as_bytes()).unwrap();
        assert_eq!(subjects.len(), 1);
        
        let s = &subjects[0];
        assert_eq!(s.source_ref, "13");
        assert_eq!(s.kind, SubjectKind::Person);
        assert_eq!(s.primary_name, "Saddam Hussein Al-Tikriti");
        assert_eq!(s.aliases.len(), 1);
        assert_eq!(s.aliases[0].name, "Abu Ali");
        assert_eq!(s.date_of_birth_year, Some(1937));
        assert_eq!(s.country, Some("IQ".to_string()));
    }
}
