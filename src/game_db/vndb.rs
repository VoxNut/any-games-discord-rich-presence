use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct VndbClient {
    client: Client,
    token: Option<String>,
}

#[derive(Debug, Serialize)]
struct VndbSearchRequest<'a> {
    filters: Vec<&'a str>,
    fields: &'static str,
    results: u32,
}

#[derive(Debug, Deserialize)]
struct VndbSearchResponse {
    results: Vec<VndbVnEntry>,
}

#[derive(Debug, Deserialize)]
struct VndbVnEntry {
    #[allow(dead_code)]
    id: String,
    title: String,
    #[serde(default)]
    #[allow(dead_code)]
    alttitle: Option<String>,
    image: Option<VndbImage>,
}

#[derive(Debug, Deserialize)]
struct VndbImage {
    url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VndbGameInfo {
    pub display_name: String,
    pub image_url: Option<String>,
}

impl VndbClient {
    pub fn new(token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();

        Self { client, token }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if let Some(ref tok) = self.token {
            let clean = tok.trim();
            if !clean.is_empty() {
                if let Ok(val) = HeaderValue::from_str(&format!("Token {}", clean)) {
                    headers.insert(AUTHORIZATION, val);
                }
            }
        }
        headers
    }

    /// Search VNDB for a visual novel by title and retrieve its cover image
    pub async fn resolve_game(&self, search_term: &str) -> Result<Option<VndbGameInfo>> {
        let endpoint = "https://api.vndb.org/kana/vn";

        let body = VndbSearchRequest {
            filters: vec!["search", "=", search_term],
            fields: "title, alttitle, image.url",
            results: 1,
        };

        debug!("Querying VNDB Kana API for '{}'", search_term);
        let resp = self
            .client
            .post(endpoint)
            .headers(self.headers())
            .json(&body)
            .send()
            .await
            .context("Failed to send request to VNDB API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            warn!("VNDB API returned HTTP {}", status);
            return Ok(None);
        }

        let search_res: VndbSearchResponse = resp
            .json()
            .await
            .context("Failed to parse VNDB JSON response")?;

        if let Some(vn) = search_res.results.into_iter().next() {
            let display_name = vn.title;
            let image_url = vn.image.and_then(|img| img.url);

            debug!("VNDB match found: '{}' (Cover: {:?})", display_name, image_url);
            Ok(Some(VndbGameInfo {
                display_name,
                image_url,
            }))
        } else {
            debug!("VNDB returned 0 results for '{}'", search_term);
            Ok(None)
        }
    }
}
