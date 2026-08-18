use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMetadata {
    pub query_key: String,
    pub display_name: String,
    pub image_url: Option<String>,
    pub small_image_url: Option<String>,
    pub provider: String,
    pub cached_at: u64,
}

pub struct MetadataCache {
    db_path: PathBuf,
}

impl MetadataCache {
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create cache directory at {:?}", cache_dir))?;

        let db_path = cache_dir.join("game_metadata_cache.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("Failed to open SQLite cache at {:?}", db_path))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS game_metadata_cache (
                query_key TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                image_url TEXT,
                small_image_url TEXT,
                provider TEXT NOT NULL,
                cached_at INTEGER NOT NULL
            );",
            [],
        )
        .context("Failed to initialize game_metadata_cache table")?;

        Ok(Self { db_path })
    }

    fn open_conn(&self) -> Result<Connection> {
        Connection::open(&self.db_path)
            .with_context(|| format!("Failed to connect to SQLite at {:?}", self.db_path))
    }

    /// Retrieve cached metadata for a query key (e.g. "eldenring", "hollow_knight")
    pub fn get(&self, query_key: &str) -> Result<Option<CachedMetadata>> {
        let conn = self.open_conn()?;
        let key = query_key.to_lowercase();

        let mut stmt = conn.prepare(
            "SELECT query_key, display_name, image_url, small_image_url, provider, cached_at
             FROM game_metadata_cache
             WHERE query_key = ?1",
        )?;

        let mut rows = stmt.query(params![key])?;

        if let Some(row) = rows.next()? {
            let metadata = CachedMetadata {
                query_key: row.get(0)?,
                display_name: row.get(1)?,
                image_url: row.get(2)?,
                small_image_url: row.get(3)?,
                provider: row.get(4)?,
                cached_at: row.get(5)?,
            };
            debug!("Cache hit for key '{}': {:?}", key, metadata.display_name);
            Ok(Some(metadata))
        } else {
            debug!("Cache miss for key '{}'", key);
            Ok(None)
        }
    }

    /// Insert or update metadata in the cache
    pub fn set(
        &self,
        query_key: &str,
        display_name: &str,
        image_url: Option<&str>,
        small_image_url: Option<&str>,
        provider: &str,
    ) -> Result<()> {
        let conn = self.open_conn()?;
        let key = query_key.to_lowercase();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        conn.execute(
            "INSERT OR REPLACE INTO game_metadata_cache (
                query_key, display_name, image_url, small_image_url, provider, cached_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![key, display_name, image_url, small_image_url, provider, now],
        )?;

        debug!(
            "Saved metadata to cache for '{}' -> '{}' (provider: {})",
            key, display_name, provider
        );
        Ok(())
    }

    /// Clear all cached entries
    pub fn clear(&self) -> Result<()> {
        let conn = self.open_conn()?;
        conn.execute("DELETE FROM game_metadata_cache", [])?;
        info!("Cleared game metadata cache");
        Ok(())
    }
}
