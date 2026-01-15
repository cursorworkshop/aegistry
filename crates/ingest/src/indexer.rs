use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, Value, STORED, STRING, TEXT};
use tantivy::{doc, Index, IndexWriter, TantivyDocument};
use unicode_normalization::UnicodeNormalization;

pub struct SearchIndex {
    index: Index,
    pub subject_id: Field,
    pub primary_name: Field,
    pub aliases: Field,
    pub country: Field,
    pub dob_year: Field,
    pub source: Field,
    pub kind: Field,
}

impl SearchIndex {
    pub fn create(index_path: &Path) -> Result<Self> {
        std::fs::create_dir_all(index_path)?;

        let mut schema_builder = Schema::builder();
        let subject_id = schema_builder.add_text_field("subject_id", STRING | STORED);
        let primary_name = schema_builder.add_text_field("primary_name", TEXT | STORED);
        let aliases = schema_builder.add_text_field("aliases", TEXT);
        let country = schema_builder.add_text_field("country", STRING | STORED);
        let dob_year = schema_builder.add_text_field("dob_year", STRING | STORED);
        let source = schema_builder.add_text_field("source", STRING | STORED);
        let kind = schema_builder.add_text_field("kind", STRING | STORED);

        let schema = schema_builder.build();
        let index = Index::create_in_dir(index_path, schema)
            .context("failed to create Tantivy index")?;

        Ok(Self {
            index,
            subject_id,
            primary_name,
            aliases,
            country,
            dob_year,
            source,
            kind,
        })
    }

    pub fn open(index_path: &Path) -> Result<Self> {
        let index = Index::open_in_dir(index_path)
            .context("failed to open Tantivy index")?;

        let schema = index.schema();
        let subject_id = schema.get_field("subject_id").unwrap();
        let primary_name = schema.get_field("primary_name").unwrap();
        let aliases = schema.get_field("aliases").unwrap();
        let country = schema.get_field("country").unwrap();
        let dob_year = schema.get_field("dob_year").unwrap();
        let source = schema.get_field("source").unwrap();
        let kind = schema.get_field("kind").unwrap();

        Ok(Self {
            index,
            subject_id,
            primary_name,
            aliases,
            country,
            dob_year,
            source,
            kind,
        })
    }

    pub fn build_from_db(&self, conn: &Connection) -> Result<usize> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        writer.delete_all_documents()?;

        let mut stmt = conn.prepare(
            r#"SELECT s.id, s.primary_name, s.country, s.date_of_birth_year, s.source, s.kind,
                      GROUP_CONCAT(a.name, ' ') as aliases
               FROM subject s
               LEFT JOIN subject_alias a ON a.subject_id = s.id
               GROUP BY s.id"#
        )?;

        let mut count = 0;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let country: Option<String> = row.get(2)?;
            let dob_year: Option<i32> = row.get(3)?;
            let source: String = row.get(4)?;
            let kind: String = row.get(5)?;
            let aliases_str: Option<String> = row.get(6)?;

            let normalized_name = normalize_for_index(&name);
            let normalized_aliases = aliases_str.as_ref().map(|a| normalize_for_index(a)).unwrap_or_default();

            writer.add_document(doc!(
                self.subject_id => id,
                self.primary_name => normalized_name,
                self.aliases => normalized_aliases,
                self.country => country.unwrap_or_default(),
                self.dob_year => dob_year.map(|y| y.to_string()).unwrap_or_default(),
                self.source => source,
                self.kind => kind,
            ))?;
            count += 1;
        }

        writer.commit()?;
        tracing::info!(count, "built search index");
        Ok(count)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let normalized_query = normalize_for_index(query);
        let query_parser = QueryParser::for_index(&self.index, vec![self.primary_name, self.aliases]);
        
        let parsed_query = query_parser.parse_query(&normalized_query)
            .unwrap_or_else(|_| {
                let term = tantivy::Term::from_field_text(self.primary_name, &normalized_query);
                Box::new(tantivy::query::FuzzyTermQuery::new(term, 2, true))
            });

        let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(limit))?;

        let mut hits = Vec::new();
        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;
            
            let subject_id = doc.get_first(self.subject_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let primary_name = doc.get_first(self.primary_name)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let country = doc.get_first(self.country)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let dob_year = doc.get_first(self.dob_year)
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok());
            let source = doc.get_first(self.source)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let kind = doc.get_first(self.kind)
                .and_then(|v| v.as_str())
                .unwrap_or("person")
                .to_string();

            hits.push(SearchHit {
                subject_id,
                primary_name,
                country,
                dob_year,
                source,
                kind,
            });
        }

        Ok(hits)
    }
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub subject_id: String,
    pub primary_name: String,
    pub country: Option<String>,
    pub dob_year: Option<i32>,
    pub source: String,
    pub kind: String,
}

fn normalize_for_index(s: &str) -> String {
    s.nfd()
        .filter(|c| !matches!(c, '\u{0300}'..='\u{036F}'))
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_accents() {
        assert_eq!(normalize_for_index("Alvaro Nunez"), "alvaro nunez");
    }
}

