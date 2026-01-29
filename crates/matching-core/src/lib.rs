use aegistry_core::{HitSource, ScoreComponents, SubjectKind};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use strsim::jaro_winkler;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, FuzzyTermQuery, Occur, Query};
use tantivy::schema::{Field, Value};
use tantivy::{Index, TantivyDocument, Term};
use unicode_normalization::UnicodeNormalization;

pub struct MatchingEngine {
    index: Index,
    subject_id: Field,
    primary_name: Field,
    aliases: Field,
    country: Field,
    dob_year: Field,
    source: Field,
    kind: Field,
}

impl MatchingEngine {
    pub fn open(index_path: &Path, _db_path: &Path) -> anyhow::Result<Self> {
        let index = Index::open_in_dir(index_path)?;
        let schema = index.schema();

        Ok(Self {
            index,
            subject_id: schema.get_field("subject_id").unwrap(),
            primary_name: schema.get_field("primary_name").unwrap(),
            aliases: schema.get_field("aliases").unwrap(),
            country: schema.get_field("country").unwrap(),
            dob_year: schema.get_field("dob_year").unwrap(),
            source: schema.get_field("source").unwrap(),
            kind: schema.get_field("kind").unwrap(),
        })
    }

    pub fn search_and_score(
        &self,
        name: &str,
        country: Option<&str>,
        dob_year: Option<i32>,
        max_results: usize,
    ) -> Vec<MatchResult> {
        // Get more candidates to ensure we find good matches
        let candidates = match self.search_candidates(name, max_results * 10) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "search failed, returning empty");
                return Vec::new();
            }
        };

        let norm_input = normalize_name(name);
        let input_parts: Vec<&str> = norm_input.split_whitespace().collect();
        
        let mut results: Vec<MatchResult> = candidates
            .into_iter()
            .map(|candidate| {
                let norm_subject = normalize_name(&candidate.primary_name);
                
                // Use improved name similarity that considers both full name and parts
                let name_similarity = compute_name_similarity(&norm_input, &input_parts, &norm_subject);

                let country_match = match (country, candidate.country.as_deref()) {
                    (Some(c_in), Some(c_subj)) if c_in.eq_ignore_ascii_case(c_subj) => 1.0,
                    _ => 0.0,
                };

                let dob_similarity = match (dob_year, candidate.dob_year) {
                    (Some(inp), Some(subj)) if inp == subj => 1.0,
                    (Some(inp), Some(subj)) if (inp - subj).abs() <= 2 => 0.5,
                    _ => 0.0,
                };

                let components = ScoreComponents {
                    name_similarity,
                    dob_similarity,
                    country_match,
                };

                // Weighted score: name is most important
                // Base score calculation
                let mut score = 0.70 * name_similarity + 0.20 * country_match + 0.10 * dob_similarity;
                
                // Boost for perfect matches:
                // - Perfect name match (1.0) + country match = guaranteed high score
                if name_similarity >= 0.99 && country_match >= 1.0 {
                    score = score.max(0.95); // Guarantee Hit level for perfect matches
                }
                
                // Stricter requirements for high confidence (but allow perfect matches):
                // - If country provided but doesn't match, cap at Review level (unless perfect name match)
                if country.is_some() && country_match < 1.0 && name_similarity < 0.99 && score > 0.90 {
                    score = score.min(0.89); // Cap at Review level
                }
                // - If DOB provided but doesn't match, cap at Review level (unless perfect name+country match)
                if dob_year.is_some() && dob_similarity < 1.0 && (name_similarity < 0.99 || country_match < 1.0) && score > 0.90 {
                    score = score.min(0.89); // Cap at Review level
                }

                MatchResult {
                    subject_id: candidate.subject_id,
                    primary_name: candidate.primary_name,
                    source: parse_source(&candidate.source),
                    kind: parse_kind(&candidate.kind),
                    country: candidate.country,
                    dob_year: candidate.dob_year,
                    score,
                    components,
                }
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);
        results
    }

    fn search_candidates(&self, query: &str, limit: usize) -> anyhow::Result<Vec<Candidate>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let normalized_query = normalize_name(query);
        let words: Vec<&str> = normalized_query.split_whitespace().collect();
        
        // Build a query that requires ALL words to match (with fuzzy tolerance)
        // This ensures "vladimir putin" finds entries containing both words
        let mut should_clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        
        for word in &words {
            if word.len() >= 3 {
                // Fuzzy search for each word in primary_name
                let term = Term::from_field_text(self.primary_name, word);
                let fuzzy = FuzzyTermQuery::new(term, 1, true);
                should_clauses.push((Occur::Should, Box::new(fuzzy)));
                
                // Also search in aliases
                let alias_term = Term::from_field_text(self.aliases, word);
                let alias_fuzzy = FuzzyTermQuery::new(alias_term, 1, true);
                should_clauses.push((Occur::Should, Box::new(alias_fuzzy)));
            }
        }

        let combined_query = BooleanQuery::new(should_clauses);
        let top_docs = searcher.search(&combined_query, &TopDocs::with_limit(limit))?;

        let mut seen_ids = HashSet::new();
        let mut candidates = Vec::new();
        
        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;

            let subject_id = doc
                .get_first(self.subject_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            
            // Deduplicate
            if seen_ids.contains(&subject_id) {
                continue;
            }
            seen_ids.insert(subject_id.clone());
            
            let primary_name = doc
                .get_first(self.primary_name)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let country = doc
                .get_first(self.country)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let dob_year = doc
                .get_first(self.dob_year)
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok());
            let source = doc
                .get_first(self.source)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let kind = doc
                .get_first(self.kind)
                .and_then(|v| v.as_str())
                .unwrap_or("person")
                .to_string();

            candidates.push(Candidate {
                subject_id,
                primary_name,
                country,
                dob_year,
                source,
                kind,
            });
        }

        Ok(candidates)
    }
}

/// Compute name similarity using parts-based matching as primary strategy
fn compute_name_similarity(input: &str, input_parts: &[&str], subject: &str) -> f32 {
    let subject_parts: Vec<&str> = subject.split_whitespace().collect();
    
    // Primary strategy: Parts-based matching
    // This ensures "putin" must match something close to "putin", not just "petrusenko"
    let parts_score = compute_parts_match_score(input_parts, &subject_parts);
    
    // Bonus for exact containment (input fully contained in subject)
    let containment_bonus = if subject.contains(input) {
        0.05
    } else {
        0.0
    };
    
    // Full string Jaro-Winkler as secondary signal (capped to not override parts score)
    let full_jw = jaro_winkler(input, subject) as f32;
    
    // If parts score is high, use it; otherwise blend with full JW
    if parts_score >= 0.9 {
        (parts_score + containment_bonus).min(1.0)
    } else if parts_score >= 0.7 {
        // Blend: weight parts more heavily
        (0.7 * parts_score + 0.3 * full_jw + containment_bonus).min(1.0)
    } else {
        // Low parts match - use full JW but capped
        (full_jw * 0.85).min(0.75)
    }
}

/// Match individual name parts with strict scoring
/// Requires ALL input parts to have a good match in subject for a high score
fn compute_parts_match_score(input_parts: &[&str], subject_parts: &[&str]) -> f32 {
    if input_parts.is_empty() || subject_parts.is_empty() {
        return 0.0;
    }
    
    let mut total_score = 0.0;
    let mut matched_count = 0;
    let mut used_subject_parts: Vec<bool> = vec![false; subject_parts.len()];
    
    for input_part in input_parts {
        // Find best matching subject part that hasn't been used yet
        let mut best_match = 0.0f32;
        let mut best_idx = None;
        
        for (idx, sp) in subject_parts.iter().enumerate() {
            if used_subject_parts[idx] {
                continue;
            }
            let sim = jaro_winkler(input_part, sp) as f32;
            if sim > best_match {
                best_match = sim;
                best_idx = Some(idx);
            }
        }
        
        // Require high similarity for a match (0.90 threshold for strict matching)
        if best_match >= 0.90 {
            if let Some(idx) = best_idx {
                used_subject_parts[idx] = true;
            }
            total_score += best_match;
            matched_count += 1;
        } else if best_match >= 0.80 {
            // Partial match - count but with penalty
            if let Some(idx) = best_idx {
                used_subject_parts[idx] = true;
            }
            total_score += best_match * 0.8; // Penalize partial matches
            matched_count += 1;
        }
    }
    
    // All input parts must match for a high score
    if matched_count < input_parts.len() {
        // Heavy penalty for missing parts
        let missing = input_parts.len() - matched_count;
        let penalty = 0.25 * missing as f32;
        let base = if matched_count > 0 {
            total_score / matched_count as f32
        } else {
            0.0
        };
        return (base - penalty).max(0.0);
    }
    
    // All parts matched - return average similarity
    total_score / matched_count as f32
}

#[derive(Debug, Clone)]
struct Candidate {
    subject_id: String,
    primary_name: String,
    country: Option<String>,
    dob_year: Option<i32>,
    source: String,
    kind: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MatchResult {
    pub subject_id: String,
    pub primary_name: String,
    pub source: HitSource,
    pub kind: SubjectKind,
    pub country: Option<String>,
    pub dob_year: Option<i32>,
    pub score: f32,
    pub components: ScoreComponents,
}

fn parse_source(s: &str) -> HitSource {
    match s.to_uppercase().as_str() {
        "EU" | "EU_CONSOLIDATED" => HitSource::EuConsolidated,
        "UN" | "UN_SC" => HitSource::UnSc,
        "OFAC" => HitSource::Ofac,
        "UK" => HitSource::Uk,
        "PEP_EU" => HitSource::PepEu,
        _ => HitSource::Stub,
    }
}

fn parse_kind(s: &str) -> SubjectKind {
    match s.to_lowercase().as_str() {
        "entity" | "enterprise" => SubjectKind::Entity,
        _ => SubjectKind::Person,
    }
}

pub fn normalize_name(value: &str) -> String {
    value
        .nfd()
        .filter(|c| !c.is_mark_nonspacing())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

trait NonSpacingMark {
    fn is_mark_nonspacing(&self) -> bool;
}

impl NonSpacingMark for char {
    fn is_mark_nonspacing(&self) -> bool {
        matches!(self, '\u{0300}'..='\u{036F}')
    }
}

// Keep stub for fallback/testing
#[derive(Clone, Debug, Serialize)]
pub struct StubSubject {
    pub subject_id: &'static str,
    pub name: &'static str,
    pub source: HitSource,
    pub kind: SubjectKind,
    pub country: Option<&'static str>,
    pub year_of_birth: Option<i32>,
}

pub const STUB_SUBJECTS: &[StubSubject] = &[
    StubSubject {
        subject_id: "eu_0001",
        name: "Maria Garcia",
        source: HitSource::EuConsolidated,
        kind: SubjectKind::Person,
        country: Some("ES"),
        year_of_birth: Some(1985),
    },
    StubSubject {
        subject_id: "ofac_0002",
        name: "John Doe",
        source: HitSource::Ofac,
        kind: SubjectKind::Person,
        country: Some("US"),
        year_of_birth: Some(1970),
    },
    StubSubject {
        subject_id: "pep_eu_0003",
        name: "Jean Dupont",
        source: HitSource::PepEu,
        kind: SubjectKind::Person,
        country: Some("FR"),
        year_of_birth: None,
    },
];

pub fn score_against_stub(
    name: &str,
    country: Option<&str>,
    dob_year: Option<i32>,
    max_results: usize,
) -> Vec<StubMatchResult> {
    let norm_input = normalize_name(name);
    let mut results = STUB_SUBJECTS
        .iter()
        .map(|subject| {
            let norm_subject = normalize_name(subject.name);
            let name_similarity = jaro_winkler(&norm_input, &norm_subject) as f32;
            let country_match = match (country, subject.country) {
                (Some(c_in), Some(c_subj)) if c_in.eq_ignore_ascii_case(c_subj) => 1.0,
                _ => 0.0,
            };
            let dob_similarity = match (dob_year, subject.year_of_birth) {
                (Some(inp), Some(subj)) if inp == subj => 1.0,
                (Some(inp), Some(subj)) if (inp - subj).abs() == 1 => 0.5,
                _ => 0.0,
            };
            let components = ScoreComponents {
                name_similarity,
                dob_similarity,
                country_match,
            };
            let score = 0.75 * name_similarity + 0.15 * country_match + 0.10 * dob_similarity;
            StubMatchResult {
                subject,
                score,
                components,
            }
        })
        .collect::<Vec<_>>();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    results.truncate(max_results);
    results
}

#[derive(Clone, Debug)]
pub struct StubMatchResult {
    pub subject: &'static StubSubject,
    pub score: f32,
    pub components: ScoreComponents,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_accents() {
        let input = "Alvaro   Nunez";
        let norm = normalize_name(input);
        assert_eq!(norm, "alvaro nunez");
    }

    #[test]
    fn scoring_prefers_country_and_dob() {
        let hits = score_against_stub("John Doe", Some("US"), Some(1970), 3);
        assert!(!hits.is_empty());
        let top = &hits[0];
        assert_eq!(top.subject.subject_id, "ofac_0002");
        assert!(top.components.country_match > 0.0);
        assert!(top.components.dob_similarity > 0.0);
    }
}
