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
struct VndbTitleEntry {
    #[serde(default)]
    lang: Option<String>,
    title: String,
    #[allow(dead_code)]
    #[serde(default)]
    latin: Option<String>,
    #[serde(default)]
    main: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct VndbVnEntry {
    #[allow(dead_code)]
    id: String,
    title: String,
    #[serde(default)]
    alttitle: Option<String>,
    #[serde(default)]
    titles: Option<Vec<VndbTitleEntry>>,
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

    /// Search VNDB for a visual novel by title and retrieve its display name and cover image
    pub async fn resolve_game(
        &self,
        search_term: &str,
        prefer_original_title: bool,
    ) -> Result<Option<VndbGameInfo>> {
        let endpoint = "https://api.vndb.org/kana/vn";

        let body = VndbSearchRequest {
            filters: vec!["search", "=", search_term],
            fields: "title, alttitle, titles.lang, titles.title, titles.latin, titles.main, image.url",
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
            let display_name = if prefer_original_title {
                // 1. Try to find the Japanese title or main native title from titles list
                let from_titles = vn.titles.as_ref().and_then(|list| {
                    list.iter()
                        .find(|t| t.lang.as_deref() == Some("ja") && !t.title.trim().is_empty())
                        .or_else(|| list.iter().find(|t| t.main == Some(true) && !t.title.trim().is_empty()))
                        .map(|t| t.title.clone())
                });

                // 2. Try alttitle (typically the original Japanese script title)
                from_titles
                    .or_else(|| vn.alttitle.filter(|alt| !alt.trim().is_empty()))
                    .unwrap_or(vn.title)
            } else {
                vn.title
            };

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
