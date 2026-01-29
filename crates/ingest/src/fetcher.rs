use anyhow::{Context, Result};
use bytes::Bytes;
use std::time::Duration;

const EU_RSS_URL: &str = "https://webgate.ec.europa.eu/fsd/fsf/public/rss";
const UN_XML_URL: &str = "https://scsanctions.un.org/resources/xml/en/consolidated.xml";
const OFAC_SDN_URL: &str = "https://www.treasury.gov/ofac/downloads/sdn.xml";
// UK Sanctions List - try multiple URLs as they change frequently
// As of 2024, UKSL is the primary source (OFSI Consolidated List deprecated Jan 2026)
const UK_SANCTIONS_URLS: &[&str] = &[
    // Try direct XML download from gov.uk publications
    "https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/UK_Sanctions_List.xml",
    // Try OFSI storage (may still work until Jan 2026)
    "https://ofsistorage.blob.core.windows.net/publishlive/ConList.xml",
    // Try alternative blob storage patterns
    "https://ofsistorage.blob.core.windows.net/publishlive/UK_Sanctions_List.xml",
    // Try CSV format as fallback (we can parse CSV too)
    "https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/UK_Sanctions_List.csv",
];

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .context("failed to build HTTP client")
}

/// Retry helper with exponential backoff
async fn retry_with_backoff<F, Fut, T>(
    mut f: F,
    max_retries: usize,
    initial_delay_ms: u64,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut last_error = None;
    
    for attempt in 0..=max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);
                if attempt < max_retries {
                    let delay_ms = initial_delay_ms * (1 << attempt); // Exponential backoff
                    tracing::warn!(
                        attempt = attempt + 1,
                        max_retries = max_retries + 1,
                        delay_ms = delay_ms,
                        "retrying after error"
                    );
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }
    
    Err(last_error.unwrap())
}

pub async fn fetch_eu_sanctions_xml() -> Result<Bytes> {
    let client = build_client()?;

    // First, fetch the RSS to get the current download URL with token
    tracing::info!(url = EU_RSS_URL, "fetching EU sanctions RSS feed");
    
    let rss_response = retry_with_backoff(
        || async {
            client
                .get(EU_RSS_URL)
                .header("Accept", "application/xml, text/xml")
                .send()
                .await
                .context("failed to fetch EU sanctions RSS")
        },
        3,
        1000, // Start with 1 second delay
    )
    .await?;

    if !rss_response.status().is_success() {
        anyhow::bail!("EU sanctions RSS fetch returned HTTP {}", rss_response.status());
    }

    let rss_text = rss_response.text().await?;
    
    // Parse RSS to find XML v1.1 URL with token
    let xml_url = extract_xml_url(&rss_text)
        .context("failed to extract XML URL from RSS feed")?;

    tracing::info!(url = %xml_url, "fetching EU consolidated sanctions list");

    let response = retry_with_backoff(
        || async {
            client
                .get(&xml_url)
                .header("Accept", "application/xml")
                .send()
                .await
                .context("failed to fetch EU sanctions XML")
        },
        3,
        2000, // Start with 2 second delay for large file
    )
    .await?;

    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("EU sanctions fetch returned HTTP {}", status);
    }

    let bytes = response
        .bytes()
        .await
        .context("failed to read response body")?;

    tracing::info!(bytes = bytes.len(), "downloaded EU sanctions XML");
    Ok(bytes)
}

pub async fn fetch_un_sanctions_xml() -> Result<Bytes> {
    let client = build_client()?;
    
    tracing::info!(url = UN_XML_URL, "fetching UN Security Council sanctions list");
    
    let response = client
        .get(UN_XML_URL)
        .header("Accept", "application/xml")
        .send()
        .await
        .context("failed to fetch UN sanctions XML")?;

    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("UN sanctions fetch returned HTTP {}", status);
    }

    let bytes = response.bytes().await.context("failed to read UN response body")?;
    tracing::info!(bytes = bytes.len(), "downloaded UN sanctions XML");
    Ok(bytes)
}

pub async fn fetch_ofac_sdn_xml() -> Result<Bytes> {
    let client = build_client()?;
    
    tracing::info!(url = OFAC_SDN_URL, "fetching OFAC SDN list");
    
    let response = client
        .get(OFAC_SDN_URL)
        .header("Accept", "application/xml")
        .send()
        .await
        .context("failed to fetch OFAC SDN XML")?;

    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("OFAC SDN fetch returned HTTP {}", status);
    }

    let bytes = response.bytes().await.context("failed to read OFAC response body")?;
    tracing::info!(bytes = bytes.len(), "downloaded OFAC SDN XML");
    Ok(bytes)
}

pub async fn fetch_uk_sanctions_xml() -> Result<Bytes> {
    let client = build_client()?;
    
    // Try XML URLs first
    for url in UK_SANCTIONS_URLS {
        if url.ends_with(".csv") {
            continue; // Try CSV later
        }
        
        tracing::info!(url = url, "trying UK Sanctions list URL");
        
        let response = match client
            .get(*url)
            .header("Accept", "application/xml, text/xml")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(url = url, error = %e, "failed to fetch, trying next URL");
                continue;
            }
        };

        let status = response.status();
        if !status.is_success() {
            tracing::warn!(url = url, status = %status, "HTTP error, trying next URL");
            continue;
        }

        let bytes = response.bytes().await.context("failed to read UK response body")?;
        if bytes.len() > 1000 {
            // Valid XML should be at least 1KB
            tracing::info!(bytes = bytes.len(), "downloaded UK sanctions XML");
            return Ok(bytes);
        }
    }
    
    // Try scraping the gov.uk page for download links
    tracing::info!("trying to scrape UK Sanctions List page for download links");
    match scrape_uk_sanctions_page(&client).await {
        Ok(Some(bytes)) => {
            tracing::info!(bytes = bytes.len(), "downloaded UK sanctions via page scrape");
            return Ok(bytes);
        }
        Ok(None) => {
            tracing::warn!("no download links found on UK sanctions page");
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to scrape UK sanctions page");
        }
    }
    
    anyhow::bail!("UK sanctions fetch failed from all URLs and page scraping")
}

pub async fn fetch_canada_sanctions() -> Result<Bytes> {
    let client = build_client()?;
    let url = "https://www.international.gc.ca/world-monde/international_relations-relations_internationales/sanctions/sema-eng.aspx";
    
    tracing::info!("fetching Canada sanctions list");
    
    // Try to fetch XML/CSV from the page
    let response = client
        .get(url)
        .send()
        .await
        .context("failed to fetch Canada sanctions page")?;
    
    if !response.status().is_success() {
        anyhow::bail!("Canada sanctions page returned HTTP {}", response.status());
    }
    
    // For now, return empty - Canada uses a different format
    // TODO: Parse HTML page to find download links
    tracing::warn!("Canada sanctions fetch not fully implemented - needs HTML parsing");
    Ok(Bytes::new())
}

pub async fn fetch_switzerland_sanctions() -> Result<Bytes> {
    let client = build_client()?;
    let url = "https://www.seco.admin.ch/seco/en/home/Aussenwirtschaftspolitik_Wirtschaftliche_Zusammenarbeit/Wirtschaftsbeziehungen/exportkontrollen-und-sanktionen/sanktionen-embargos.html";
    
    tracing::info!("fetching Switzerland sanctions list");
    
    let response = client
        .get(url)
        .send()
        .await
        .context("failed to fetch Switzerland sanctions page")?;
    
    if !response.status().is_success() {
        anyhow::bail!("Switzerland sanctions page returned HTTP {}", response.status());
    }
    
    // For now, return empty - Switzerland uses a different format
    tracing::warn!("Switzerland sanctions fetch not fully implemented - needs HTML parsing");
    Ok(Bytes::new())
}

pub async fn fetch_australia_sanctions() -> Result<Bytes> {
    let client = build_client()?;
    let url = "https://www.dfat.gov.au/international-relations/security/sanctions";
    
    tracing::info!("fetching Australia sanctions list");
    
    let response = client
        .get(url)
        .send()
        .await
        .context("failed to fetch Australia sanctions page")?;
    
    if !response.status().is_success() {
        anyhow::bail!("Australia sanctions page returned HTTP {}", response.status());
    }
    
    // For now, return empty - Australia uses a different format
    tracing::warn!("Australia sanctions fetch not fully implemented - needs HTML parsing");
    Ok(Bytes::new())
}

async fn scrape_uk_sanctions_page(client: &reqwest::Client) -> Result<Option<Bytes>> {
    let page_url = "https://www.gov.uk/government/publications/the-uk-sanctions-list";
    
    let response = client
        .get(page_url)
        .header("Accept", "text/html")
        .send()
        .await
        .context("failed to fetch UK sanctions page")?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let html = response.text().await?;
    
    // Look for XML download links in the HTML
    // Pattern: href="...UK_Sanctions_List.xml" or href="...ConList.xml"
    for line in html.lines() {
        if line.contains("UK_Sanctions_List") || line.contains("ConList") {
            if let Some(url_start) = line.find("href=\"") {
                let rest = &line[url_start + 6..];
                if let Some(url_end) = rest.find('"') {
                    let mut url = rest[..url_end].to_string();
                    
                    // Make absolute URL if relative
                    if url.starts_with("/") {
                        url = format!("https://www.gov.uk{}", url);
                    } else if !url.starts_with("http") {
                        url = format!("https://www.gov.uk{}", url);
                    }
                    
                    tracing::info!(url = %url, "found download link, attempting fetch");
                    
                    let dl_response = client
                        .get(&url)
                        .header("Accept", "application/xml, text/xml")
                        .send()
                        .await;
                    
                    if let Ok(resp) = dl_response {
                        if resp.status().is_success() {
                            let bytes = resp.bytes().await?;
                            if bytes.len() > 1000 {
                                return Ok(Some(bytes));
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(None)
}

fn extract_xml_url(rss: &str) -> Option<String> {
    // Look for XML v1.1 URL in the RSS
    // Pattern: <link>https://...xmlFullSanctionsList_1_1/content?token=...</link>
    for line in rss.lines() {
        let line = line.trim();
        if line.contains("xmlFullSanctionsList_1_1") && line.contains("token=") {
            // Extract URL from <link>...</link> or enclosure url="..."
            if let Some(start) = line.find("https://") {
                let rest = &line[start..];
                if let Some(end) = rest.find(|c| c == '<' || c == '"' || c == '&') {
                    return Some(rest[..end].to_string());
                }
            }
        }
    }
    None
}

pub fn compute_sha256(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        let data = b"test data";
        let h1 = compute_sha256(data);
        let h2 = compute_sha256(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn extract_url_from_rss() {
        let rss = r#"
        <item>
          <title>XML (Based on XSD) - v1.1</title>
          <link>https://webgate.ec.europa.eu/fsd/fsf/public/files/xmlFullSanctionsList_1_1/content?token=dG9rZW4tMjAxNw</link>
        </item>
        "#;
        let url = extract_xml_url(rss).unwrap();
        assert!(url.contains("xmlFullSanctionsList_1_1"));
        assert!(url.contains("token="));
    }
}
