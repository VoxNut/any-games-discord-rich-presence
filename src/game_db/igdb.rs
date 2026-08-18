use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct IgdbClient {
    client: Client,
    client_id: String,
    client_secret: String,
    token_state: Arc<RwLock<Option<TokenState>>>,
}

#[derive(Debug, Clone)]
struct TokenState {
    access_token: String,
    expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct TwitchTokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct IgdbGameResult {
    name: String,
    cover: Option<IgdbCover>,
}

#[derive(Debug, Deserialize)]
struct IgdbCover {
    image_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IgdbGameInfo {
    pub display_name: String,
    pub image_url: Option<String>,
}

impl IgdbClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();

        Self {
            client,
            client_id,
            client_secret,
            token_state: Arc::new(RwLock::new(None)),
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.client_id.trim().is_empty() && !self.client_secret.trim().is_empty()
    }

    async fn get_access_token(&self) -> Result<String> {
        {
            let read = self.token_state.read().await;
            if let Some(ref state) = *read {
                if Instant::now() < state.expires_at {
                    return Ok(state.access_token.clone());
                }
            }
        }

        // Fetch new token from Twitch OAuth
        let mut write = self.token_state.write().await;
        // Double check after acquiring write lock
        if let Some(ref state) = *write {
            if Instant::now() < state.expires_at {
                return Ok(state.access_token.clone());
            }
        }

        let token_url = format!(
            "https://id.twitch.tv/oauth2/token?client_id={}&client_secret={}&grant_type=client_credentials",
            self.client_id.trim(),
            self.client_secret.trim()
        );

        let resp = self
            .client
            .post(&token_url)
            .send()
            .await
            .context("Failed to request Twitch OAuth token")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Twitch OAuth token request failed with HTTP {}: {}", status, text);
        }

        let data: TwitchTokenResponse = resp.json().await.context("Failed to parse Twitch OAuth token response")?;
        let ttl = if data.expires_in > 60 {
            Duration::from_secs(data.expires_in - 60)
        } else {
            Duration::from_secs(data.expires_in)
        };

        let token = data.access_token.clone();
        *write = Some(TokenState {
            access_token: data.access_token,
            expires_at: Instant::now() + ttl,
        });

        debug!("Acquired fresh IGDB Twitch OAuth token");
        Ok(token)
    }

    pub async fn resolve_game(&self, search_term: &str) -> Result<Option<IgdbGameInfo>> {
        if !self.is_configured() {
            return Ok(None);
        }

        let token = match self.get_access_token().await {
            Ok(t) => t,
            Err(e) => {
                warn!("IGDB authentication error: {:#}", e);
                return Ok(None);
            }
        };

        let query = format!(
            "search \"{}\"; fields name, cover.image_id; limit 1;",
            search_term.replace('\"', "\\\"")
        );

        let resp = self
            .client
            .post("https://api.igdb.com/v4/games")
            .header("Client-ID", self.client_id.trim())
            .header("Authorization", format!("Bearer {}", token))
            .body(query)
            .send()
            .await
            .context("Failed to query IGDB games endpoint")?;

        if !resp.status().is_success() {
            let status = resp.status();
            warn!("IGDB query returned HTTP {}", status);
            return Ok(None);
        }

        let games: Vec<IgdbGameResult> = resp.json().await.context("Failed to parse IGDB games response")?;

        if let Some(game) = games.into_iter().next() {
            let image_url = game
                .cover
                .and_then(|c| c.image_id)
                .map(|img_id| format!("https://images.igdb.com/igdb/image/upload/t_cover_big/{}.jpg", img_id));

            Ok(Some(IgdbGameInfo {
                display_name: game.name,
                image_url,
            }))
        } else {
            Ok(None)
        }
    }
}
