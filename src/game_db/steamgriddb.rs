use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct SteamGridDbClient {
    client: Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
struct SgdbResponse<T> {
    #[allow(dead_code)]
    success: bool,
    data: Option<T>,
    #[allow(dead_code)]
    errors: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct SgdbGame {
    id: u64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct SgdbGrid {
    url: String,
}

#[derive(Debug, Deserialize)]
struct SgdbIcon {
    url: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedGameInfo {
    pub display_name: String,
    pub image_url: Option<String>,
    pub icon_url: Option<String>,
}

impl SteamGridDbClient {
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();

        Self { client, api_key }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {}", self.api_key.trim())) {
            headers.insert(AUTHORIZATION, val);
        }
        headers
    }

    /// Search for game by name and fetch highest rated cover art grid and icon
    pub async fn resolve_game(&self, search_term: &str) -> Result<Option<ResolvedGameInfo>> {
        if self.api_key.trim().is_empty() {
            return Ok(None);
        }

        let encoded_term = urlencoding::encode(search_term);
        let search_url = format!(
            "https://www.steamgriddb.com/api/v2/search/autocomplete/{}",
            encoded_term
        );

        debug!("Querying SteamGridDB for '{}'", search_term);
        let resp = self
            .client
            .get(&search_url)
            .headers(self.headers())
            .send()
            .await
            .context("Failed to send request to SteamGridDB search API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            warn!("SteamGridDB search returned HTTP {}", status);
            return Ok(None);
        }

        let result: SgdbResponse<Vec<SgdbGame>> = resp
            .json()
            .await
            .context("Failed to parse SteamGridDB search response")?;

        let game = match result.data.and_then(|list| list.into_iter().next()) {
            Some(g) => g,
            None => {
                debug!("SteamGridDB returned 0 results for '{}'", search_term);
                return Ok(None);
            }
        };

        let game_id = game.id;
        let display_name = game.name;

        // Fetch Grid (Cover Art)
        let grid_url = format!(
            "https://www.steamgriddb.com/api/v2/grids/game/{}?dimensions=600x900,512x512,1024x1024,920x430&styles=alternate,official&types=static",
            game_id
        );

        let image_url = if let Ok(resp) = self
            .client
            .get(&grid_url)
            .headers(self.headers())
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(res) = resp.json::<SgdbResponse<Vec<SgdbGrid>>>().await {
                    res.data.and_then(|mut grids| {
                        if !grids.is_empty() {
                            Some(grids.remove(0).url)
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Fetch Icon (Small Image Badge)
        let icon_url_endpoint = format!("https://www.steamgriddb.com/api/v2/icons/game/{}", game_id);
        let icon_url = if let Ok(resp) = self
            .client
            .get(&icon_url_endpoint)
            .headers(self.headers())
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(res) = resp.json::<SgdbResponse<Vec<SgdbIcon>>>().await {
                    res.data.and_then(|mut icons| {
                        if !icons.is_empty() {
                            Some(icons.remove(0).url)
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(Some(ResolvedGameInfo {
            display_name,
            image_url,
            icon_url,
        }))
    }
}
