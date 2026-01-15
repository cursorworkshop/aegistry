use anyhow::Result;
use ingest::{
    compute_sha256, fetch_eu_commission, fetch_eu_parliament_meps, fetch_eu_sanctions_xml,
    fetch_ofac_sdn_xml, fetch_uk_sanctions_xml, fetch_un_sanctions_xml, init_monitoring_schema,
    init_schema, open_db, parse_eu_xml, parse_ofac_xml, parse_uk_xml, parse_un_xml,
    record_dataset_version, upsert_subjects, SearchIndex,
    fetch_us_congress, fetch_uk_parliament, fetch_german_bundestag,
    fetch_french_assemblee, fetch_dutch_tweede_kamer,
    fetch_austria_parliament, fetch_belgium_parliament, fetch_spain_congress,
    fetch_canada_sanctions, fetch_switzerland_sanctions, fetch_australia_sanctions,
    parse_canada_sanctions, parse_switzerland_sanctions, parse_australia_sanctions,
};
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const DATA_DIR: &str = "data";
const DB_FILE: &str = "aegistry.db";
const INDEX_DIR: &str = "index";

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let data_path = PathBuf::from(DATA_DIR);
    let db_path = data_path.join(DB_FILE);
    let index_path = data_path.join(INDEX_DIR);

    tracing::info!("Aegistry Ingest - Global Sanctions Lists");
    tracing::info!(db = %db_path.display(), index = %index_path.display(), "data paths");

    // Open/create database
    let conn = open_db(&db_path)?;
    init_schema(&conn)?;
    init_monitoring_schema(&conn)?;
    tracing::info!("database initialized");

    let mut total_subjects = 0;

    // 1. Fetch and parse EU sanctions
    tracing::info!("--- EU Consolidated Sanctions List ---");
    match fetch_and_ingest_eu(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "EU sanctions loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest EU sanctions"),
    }

    // 2. Fetch and parse UN sanctions
    tracing::info!("--- UN Security Council Sanctions List ---");
    match fetch_and_ingest_un(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "UN sanctions loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest UN sanctions"),
    }

    // 3. Fetch and parse OFAC SDN
    tracing::info!("--- OFAC SDN List ---");
    match fetch_and_ingest_ofac(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "OFAC SDN loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest OFAC SDN"),
    }

    // 4. Fetch and parse UK sanctions
    tracing::info!("--- UK Sanctions List ---");
    match fetch_and_ingest_uk(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "UK sanctions loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest UK sanctions"),
    }

    // 5. Fetch and parse Canada sanctions
    tracing::info!("--- Canada Sanctions List ---");
    match fetch_and_ingest_canada(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "Canada sanctions loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest Canada sanctions"),
    }

    // 6. Fetch and parse Switzerland sanctions
    tracing::info!("--- Switzerland Sanctions List ---");
    match fetch_and_ingest_switzerland(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "Switzerland sanctions loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest Switzerland sanctions"),
    }

    // 7. Fetch and parse Australia sanctions
    tracing::info!("--- Australia Sanctions List ---");
    match fetch_and_ingest_australia(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "Australia sanctions loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest Australia sanctions"),
    }

    // 5. Fetch PEP data - EU Parliament
    tracing::info!("--- EU Parliament MEPs (PEP) ---");
    match fetch_and_ingest_eu_parliament(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "EU Parliament MEPs loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest EU Parliament MEPs"),
    }

    // 6. Fetch PEP data - European Commission
    tracing::info!("--- European Commission (PEP) ---");
    match fetch_and_ingest_eu_commission(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "European Commission members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest European Commission"),
    }

    // 10. Fetch PEP data - US Congress
    tracing::info!("--- US Congress (PEP) ---");
    match fetch_and_ingest_us_congress(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "US Congress members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest US Congress"),
    }

    // 11. Fetch PEP data - UK Parliament
    tracing::info!("--- UK Parliament (PEP) ---");
    match fetch_and_ingest_uk_parliament(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "UK Parliament members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest UK Parliament"),
    }

    // 12. Fetch PEP data - German Bundestag
    tracing::info!("--- German Bundestag (PEP) ---");
    match fetch_and_ingest_german_bundestag(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "German Bundestag members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest German Bundestag"),
    }

    // 13. Fetch PEP data - French Assemblée Nationale
    tracing::info!("--- French Assemblée Nationale (PEP) ---");
    match fetch_and_ingest_french_assemblee(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "French Assemblée Nationale members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest French Assemblée Nationale"),
    }

    // 14. Fetch PEP data - Dutch Tweede Kamer
    tracing::info!("--- Dutch Tweede Kamer (PEP) ---");
    match fetch_and_ingest_dutch_tweede_kamer(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "Dutch Tweede Kamer members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest Dutch Tweede Kamer"),
    }

    // 15. Fetch PEP data - Austria Parliament
    tracing::info!("--- Austria Parliament (PEP) ---");
    match fetch_and_ingest_austria(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "Austria Parliament members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest Austria Parliament"),
    }

    // 16. Fetch PEP data - Belgium Parliament
    tracing::info!("--- Belgium Parliament (PEP) ---");
    match fetch_and_ingest_belgium(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "Belgium Parliament members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest Belgium Parliament"),
    }

    // 17. Fetch PEP data - Spain Congress
    tracing::info!("--- Spain Congress (PEP) ---");
    match fetch_and_ingest_spain(&conn).await {
        Ok(count) => {
            total_subjects += count;
            tracing::info!(count, "Spain Congress members loaded");
        }
        Err(e) => tracing::error!(error = %e, "failed to ingest Spain Congress"),
    }

    tracing::info!(total = total_subjects, "total subjects in database");

    // Build search index
    tracing::info!("--- Building Search Index ---");
    if index_path.exists() {
        std::fs::remove_dir_all(&index_path)?;
    }
    let index = SearchIndex::create(&index_path)?;
    let indexed = index.build_from_db(&conn)?;
    tracing::info!(indexed, "search index built");

    // Test search
    tracing::info!("testing search...");
    let test_hits = index.search("putin", 5)?;
    for hit in &test_hits {
        tracing::info!(
            id = %hit.subject_id,
            name = %hit.primary_name,
            country = ?hit.country,
            "search hit"
        );
    }

    // Re-screen all monitored subjects to detect changes
    tracing::info!("re-screening monitored subjects...");
    re_screen_monitored_subjects(&conn, &index)?;

    tracing::info!("ingest complete");
    Ok(())
}

fn re_screen_monitored_subjects(conn: &rusqlite::Connection, index: &ingest::indexer::SearchIndex) -> Result<()> {
    use ingest::monitoring::{get_all_active_subjects, record_monitoring_result, compute_result_hash};
    
    let subjects = match get_all_active_subjects(conn) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to get monitored subjects, skipping re-screening");
            return Ok(());
        }
    };

    if subjects.is_empty() {
        tracing::info!("no monitored subjects to re-screen");
        return Ok(());
    }

    tracing::info!(count = subjects.len(), "re-screening monitored subjects");

    for subject in subjects {
        // Perform search
        let search_hits = match index.search(&subject.name, 10) {
            Ok(hits) => hits,
            Err(e) => {
                tracing::warn!(error = %e, reference_id = %subject.reference_id, "search failed");
                continue;
            }
        };

        // Build hit data for hash computation
        let hit_data: Vec<(String, f32)> = search_hits
            .iter()
            .map(|h| (h.subject_id.clone(), 0.9)) // Simplified score
            .collect();
        
        let new_hash = compute_result_hash(&hit_data);
        let has_changes = subject.last_result_hash.as_ref().map(|h| h != &new_hash).unwrap_or(true);
        
        // Record result
        if let Err(e) = record_monitoring_result(
            conn,
            subject.id,
            &new_hash,
            search_hits.len(),
            search_hits.first().map(|_| 0.9).unwrap_or(0.0),
            has_changes,
        ) {
            tracing::warn!(error = %e, reference_id = %subject.reference_id, "failed to record monitoring result");
        } else if has_changes {
            tracing::info!(
                reference_id = %subject.reference_id,
                hit_count = search_hits.len(),
                "detected changes for monitored subject"
            );
        }
    }

    Ok(())
}

async fn fetch_and_ingest_eu(conn: &rusqlite::Connection) -> Result<usize> {
    let xml_bytes = fetch_eu_sanctions_xml().await?;
    let file_hash = compute_sha256(&xml_bytes);
    
    let subjects = parse_eu_xml(&xml_bytes)?;
    if subjects.is_empty() {
        tracing::warn!("no EU subjects parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "EU")?;
    record_dataset_version(conn, "EU", count as i64, Some(&file_hash))?;
    Ok(count)
}

async fn fetch_and_ingest_un(conn: &rusqlite::Connection) -> Result<usize> {
    let xml_bytes = fetch_un_sanctions_xml().await?;
    let file_hash = compute_sha256(&xml_bytes);
    
    let subjects = parse_un_xml(&xml_bytes)?;
    if subjects.is_empty() {
        tracing::warn!("no UN subjects parsed");
        return Ok(0);
    }

    // Convert to ParsedSubject format
    let count = upsert_subjects(conn, &subjects, "UN")?;
    record_dataset_version(conn, "UN", count as i64, Some(&file_hash))?;
    Ok(count)
}

async fn fetch_and_ingest_ofac(conn: &rusqlite::Connection) -> Result<usize> {
    let xml_bytes = fetch_ofac_sdn_xml().await?;
    let file_hash = compute_sha256(&xml_bytes);
    
    let subjects = parse_ofac_xml(&xml_bytes)?;
    if subjects.is_empty() {
        tracing::warn!("no OFAC subjects parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "OFAC")?;
    record_dataset_version(conn, "OFAC", count as i64, Some(&file_hash))?;
    Ok(count)
}

async fn fetch_and_ingest_uk(conn: &rusqlite::Connection) -> Result<usize> {
    let xml_bytes = fetch_uk_sanctions_xml().await?;
    let file_hash = compute_sha256(&xml_bytes);
    
    let subjects = parse_uk_xml(&xml_bytes)?;
    if subjects.is_empty() {
        tracing::warn!("no UK subjects parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "UK")?;
    record_dataset_version(conn, "UK", count as i64, Some(&file_hash))?;
    Ok(count)
}

async fn fetch_and_ingest_eu_parliament(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_eu_parliament_meps().await?;
    if subjects.is_empty() {
        tracing::warn!("no EU Parliament MEPs parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "PEP_EU_PARLIAMENT")?;
    record_dataset_version(conn, "PEP_EU_PARLIAMENT", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_eu_commission(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_eu_commission().await?;
    if subjects.is_empty() {
        tracing::warn!("no European Commission members parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "PEP_EU_COMMISSION")?;
    record_dataset_version(conn, "PEP_EU_COMMISSION", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_us_congress(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_us_congress().await?;
    if subjects.is_empty() {
        tracing::warn!("no US Congress members parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "PEP_US_CONGRESS")?;
    record_dataset_version(conn, "PEP_US_CONGRESS", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_uk_parliament(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_uk_parliament().await?;
    if subjects.is_empty() {
        tracing::warn!("no UK Parliament members parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "PEP_UK_PARLIAMENT")?;
    record_dataset_version(conn, "PEP_UK_PARLIAMENT", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_german_bundestag(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_german_bundestag().await?;
    if subjects.is_empty() {
        tracing::warn!("no German Bundestag members parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "PEP_DE_BUNDESTAG")?;
    record_dataset_version(conn, "PEP_DE_BUNDESTAG", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_french_assemblee(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_french_assemblee().await?;
    if subjects.is_empty() {
        tracing::warn!("no French Assemblée Nationale members parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "PEP_FR_ASSEMBLEE")?;
    record_dataset_version(conn, "PEP_FR_ASSEMBLEE", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_dutch_tweede_kamer(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_dutch_tweede_kamer().await?;
    if subjects.is_empty() {
        tracing::warn!("no Dutch Tweede Kamer members parsed");
        return Ok(0);
    }

    let count = upsert_subjects(conn, &subjects, "PEP_NL_TWEEDE_KAMER")?;
    record_dataset_version(conn, "PEP_NL_TWEEDE_KAMER", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_austria(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_austria_parliament().await?;
    if subjects.is_empty() {
        return Ok(0);
    }
    let count = upsert_subjects(conn, &subjects, "PEP_AT")?;
    Ok(count)
}

async fn fetch_and_ingest_belgium(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_belgium_parliament().await?;
    if subjects.is_empty() {
        return Ok(0);
    }
    let count = upsert_subjects(conn, &subjects, "PEP_BE")?;
    Ok(count)
}

async fn fetch_and_ingest_spain(conn: &rusqlite::Connection) -> Result<usize> {
    let subjects = fetch_spain_congress().await?;
    if subjects.is_empty() {
        return Ok(0);
    }
    let count = upsert_subjects(conn, &subjects, "PEP_ES")?;
    Ok(count)
}

async fn fetch_and_ingest_canada(conn: &rusqlite::Connection) -> Result<usize> {
    let xml_bytes = fetch_canada_sanctions().await?;
    if xml_bytes.is_empty() {
        return Ok(0);
    }
    let subjects = parse_canada_sanctions(&xml_bytes)?;
    if subjects.is_empty() {
        return Ok(0);
    }
    let count = upsert_subjects(conn, &subjects, "CANADA")?;
    record_dataset_version(conn, "CANADA", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_switzerland(conn: &rusqlite::Connection) -> Result<usize> {
    let xml_bytes = fetch_switzerland_sanctions().await?;
    if xml_bytes.is_empty() {
        return Ok(0);
    }
    let subjects = parse_switzerland_sanctions(&xml_bytes)?;
    if subjects.is_empty() {
        return Ok(0);
    }
    let count = upsert_subjects(conn, &subjects, "SWITZERLAND")?;
    record_dataset_version(conn, "SWITZERLAND", count as i64, None)?;
    Ok(count)
}

async fn fetch_and_ingest_australia(conn: &rusqlite::Connection) -> Result<usize> {
    let xml_bytes = fetch_australia_sanctions().await?;
    if xml_bytes.is_empty() {
        return Ok(0);
    }
    let subjects = parse_australia_sanctions(&xml_bytes)?;
    if subjects.is_empty() {
        return Ok(0);
    }
    let count = upsert_subjects(conn, &subjects, "AUSTRALIA")?;
    record_dataset_version(conn, "AUSTRALIA", count as i64, None)?;
    Ok(count)
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
