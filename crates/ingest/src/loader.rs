use crate::parser_eu::{ParsedSubject, SubjectKind};
use anyhow::Result;
use rusqlite::{params, Connection};

pub fn upsert_subjects(conn: &Connection, subjects: &[ParsedSubject], source: &str) -> Result<usize> {
    let mut inserted = 0;
    let mut updated = 0;

    for subject in subjects {
        let subject_id = format!("{}_{}", source.to_lowercase(), subject.source_ref);
        let kind_str = match subject.kind {
            SubjectKind::Person => "person",
            SubjectKind::Entity => "entity",
        };

        let exists: bool = conn.query_row(
            "SELECT 1 FROM subject WHERE id = ?1",
            params![&subject_id],
            |_| Ok(true),
        ).unwrap_or(false);

        if exists {
            conn.execute(
                r#"UPDATE subject SET 
                    primary_name = ?2,
                    date_of_birth = ?3,
                    date_of_birth_year = ?4,
                    country = ?5,
                    updated_at = datetime('now')
                WHERE id = ?1"#,
                params![
                    &subject_id,
                    &subject.primary_name,
                    &subject.date_of_birth,
                    &subject.date_of_birth_year,
                    &subject.country,
                ],
            )?;
            updated += 1;
        } else {
            conn.execute(
                r#"INSERT INTO subject (id, kind, primary_name, date_of_birth, date_of_birth_year, country, source, source_ref)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
                params![
                    &subject_id,
                    kind_str,
                    &subject.primary_name,
                    &subject.date_of_birth,
                    &subject.date_of_birth_year,
                    &subject.country,
                    source,
                    &subject.source_ref,
                ],
            )?;
            inserted += 1;
        }

        conn.execute(
            "DELETE FROM subject_alias WHERE subject_id = ?1",
            params![&subject_id],
        )?;

        for alias in &subject.aliases {
            conn.execute(
                "INSERT OR IGNORE INTO subject_alias (subject_id, name, alias_type) VALUES (?1, ?2, ?3)",
                params![&subject_id, &alias.name, &alias.alias_type],
            )?;
        }
    }

    tracing::info!(inserted, updated, "upserted subjects into database");
    Ok(inserted + updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init_schema, open_db};
    use crate::parser_eu::ParsedAlias;
    use std::path::PathBuf;

    #[test]
    fn upsert_inserts_and_updates() {
        let conn = open_db(&PathBuf::from(":memory:")).unwrap();
        init_schema(&conn).unwrap();

        let subjects = vec![ParsedSubject {
            source_ref: "test_001".to_string(),
            kind: SubjectKind::Person,
            primary_name: "Test Person".to_string(),
            aliases: vec![ParsedAlias {
                name: "TP".to_string(),
                alias_type: "aka".to_string(),
            }],
            date_of_birth: Some("1980-01-01".to_string()),
            date_of_birth_year: Some(1980),
            country: Some("US".to_string()),
            nationalities: vec!["US".to_string()],
        }];

        let count = upsert_subjects(&conn, &subjects, "EU").unwrap();
        assert_eq!(count, 1);

        let count = upsert_subjects(&conn, &subjects, "EU").unwrap();
        assert_eq!(count, 1);
    }
}

