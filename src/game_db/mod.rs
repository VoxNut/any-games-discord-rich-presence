pub mod igdb;
pub mod sanitizer;
pub mod steamgriddb;
pub mod vndb;

use crate::cache::MetadataCache;
use crate::config::AppConfig;
use anyhow::Result;
use igdb::IgdbClient;
use sanitizer::sanitize_to_display_title;
use steamgriddb::SteamGridDbClient;
use vndb::VndbClient;
use std::sync::Arc;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct GameMetadata {
    pub display_name: String,
    pub image_url: Option<String>,
    pub small_image_url: Option<String>,
    pub source: String,
}

pub struct GameResolver {
    cache: Arc<MetadataCache>,
    vndb: Option<VndbClient>,
    steamgriddb: Option<SteamGridDbClient>,
    igdb: Option<IgdbClient>,
}

impl GameResolver {
    pub fn new(config: &AppConfig, cache: Arc<MetadataCache>) -> Self {
        // VNDB is enabled if token is provided or default enabled for visual novel lookups
        let vndb = config
            .api
            .vndb_token
            .as_ref()
            .map(|tok| VndbClient::new(Some(tok.clone())))
            .or_else(|| Some(VndbClient::new(None)));

        let steamgriddb = config
            .api
            .steamgriddb_api_key
            .as_ref()
            .filter(|k| !k.trim().is_empty())
            .map(|k| SteamGridDbClient::new(k.clone()));

        let igdb = match (&config.api.igdb_client_id, &config.api.igdb_client_secret) {
            (Some(cid), Some(sec)) if !cid.trim().is_empty() && !sec.trim().is_empty() => {
                Some(IgdbClient::new(cid.clone(), sec.clone()))
            }
            _ => None,
        };

        Self {
            cache,
            vndb,
            steamgriddb,
            igdb,
        }
    }

    /// Resolve metadata for a game executable, optionally using parent folder name
    pub async fn resolve(
        &self,
        exe_name: &str,
        clean_name: &str,
        folder_name: Option<&str>,
        config: &AppConfig,
    ) -> Result<GameMetadata> {
        // 1. Check manual config override first (e.g. [games.sysheep] or [games.sayonara])
        if let Some(game_ov) = config.find_game_override(exe_name) {
            if let Some(ref custom_name) = game_ov.display_name {
                let image_url = game_ov.image_url.clone();
                let small_image_url = game_ov.small_image_url.clone();
                debug!("Resolved metadata from manual config override for '{}'", exe_name);
                return Ok(GameMetadata {
                    display_name: custom_name.clone(),
                    image_url,
                    small_image_url,
                    source: "manual_config".to_string(),
                });
            }
        }

        // 2. Determine search terms:
        // If the binary is a generic engine name (like sysheep, krkr, game) and folder_name exists,
        // prioritize folder_name first
        let is_generic_engine = is_generic_engine(clean_name);
        let mut search_candidates = Vec::new();

        if is_generic_engine {
            if let Some(f) = folder_name {
                if !f.trim().is_empty() {
                    search_candidates.push(f.to_string());
                }
            }
            search_candidates.push(sanitize_to_display_title(clean_name));
        } else {
            search_candidates.push(sanitize_to_display_title(clean_name));
            if let Some(f) = folder_name {
                if !f.trim().is_empty() && f != clean_name {
                    search_candidates.push(f.to_string());
                }
            }
        }

        // Check cache with the primary candidate key
        let primary_key = search_candidates.first().cloned().unwrap_or_else(|| clean_name.to_string());
        if let Ok(Some(cached)) = self.cache.get(&primary_key) {
            debug!("Resolved metadata from local SQLite cache for '{}'", primary_key);
            return Ok(GameMetadata {
                display_name: cached.display_name,
                image_url: cached.image_url,
                small_image_url: cached.small_image_url,
                source: format!("cache ({})", cached.provider),
            });
        }

        let prefer_original = config.general.prefer_original_title;

        // 3. Try VNDB across candidates
        if let Some(ref vndb) = self.vndb {
            for query in &search_candidates {
                match vndb.resolve_game(query, prefer_original).await {
                    Ok(Some(info)) => {
                        info!("VNDB resolved '{}' -> '{}'", query, info.display_name);
                        let _ = self.cache.set(
                            &primary_key,
                            &info.display_name,
                            info.image_url.as_deref(),
                            None,
                            "vndb",
                        );
                        return Ok(GameMetadata {
                            display_name: info.display_name,
                            image_url: info.image_url,
                            small_image_url: None,
                            source: "vndb".to_string(),
                        });
                    }
                    Ok(None) => {
                        debug!("VNDB found no match for query '{}'", query);
                    }
                    Err(e) => {
                        warn!("VNDB lookup error for '{}': {:#}", query, e);
                    }
                }
            }
        }

        // 4. Try SteamGridDB across candidates
        if let Some(ref sgdb) = self.steamgriddb {
            for query in &search_candidates {
                match sgdb.resolve_game(query).await {
                    Ok(Some(info)) => {
                        info!("SteamGridDB resolved '{}' -> '{}'", query, info.display_name);
                        let _ = self.cache.set(
                            &primary_key,
                            &info.display_name,
                            info.image_url.as_deref(),
                            info.icon_url.as_deref(),
                            "steamgriddb",
                        );
                        return Ok(GameMetadata {
                            display_name: info.display_name,
                            image_url: info.image_url,
                            small_image_url: info.icon_url,
                            source: "steamgriddb".to_string(),
                        });
                    }
                    Ok(None) => {
                        debug!("SteamGridDB found no match for query '{}'", query);
                    }
                    Err(e) => {
                        warn!("SteamGridDB lookup error for '{}': {:#}", query, e);
                    }
                }
            }
        }

        // 5. Try IGDB across candidates
        if let Some(ref igdb) = self.igdb {
            for query in &search_candidates {
                match igdb.resolve_game(query).await {
                    Ok(Some(info)) => {
                        info!("IGDB resolved '{}' -> '{}'", query, info.display_name);
                        let _ = self.cache.set(
                            &primary_key,
                            &info.display_name,
                            info.image_url.as_deref(),
                            None,
                            "igdb",
                        );
                        return Ok(GameMetadata {
                            display_name: info.display_name,
                            image_url: info.image_url,
                            small_image_url: None,
                            source: "igdb".to_string(),
                        });
                    }
                    Ok(None) => {
                        debug!("IGDB found no match for query '{}'", query);
                    }
                    Err(e) => {
                        warn!("IGDB lookup error for '{}': {:#}", query, e);
                    }
                }
            }
        }

        // 6. Fallback: Sanitized title
        let fallback_title = if let Some(f) = folder_name {
            if is_generic_engine {
                f.to_string()
            } else {
                sanitize_to_display_title(clean_name)
            }
        } else {
            sanitize_to_display_title(clean_name)
        };

        info!("Using fallback title '{}' for '{}'", fallback_title, clean_name);
        let _ = self.cache.set(
            &primary_key,
            &fallback_title,
            None,
            None,
            "fallback",
        );

        Ok(GameMetadata {
            display_name: fallback_title,
            image_url: None,
            small_image_url: None,
            source: "fallback".to_string(),
        })
    }
}

fn is_generic_engine(clean_name: &str) -> bool {
    let generic = [
        "sysheep", "krkr", "tvpwin32", "rio", "game", "main", "start",
        "siglusengine", "reallive", "advhd", "bgi", "sysengine", "cs2",
        "majirov3", "artemis", "onscripter", "nscripter", "yu-ris", "ysbin",
    ];
    generic.contains(&clean_name.to_lowercase().as_str())
}
