use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "voxnut";
const APPLICATION: &str = "any-games-discord-rich-presence";

/// Top-level application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// Global settings
    #[serde(default)]
    pub general: GeneralConfig,

    /// API Keys for metadata lookups
    #[serde(default)]
    pub api: ApiConfig,

    /// Executable names to explicitly ignore (case-insensitive)
    #[serde(default = "default_ignored_processes")]
    pub ignored_processes: Vec<String>,

    /// Per-game manual configuration overrides
    /// Key: executable name (e.g. "eldenring", "witcher3.exe", "hollow_knight")
    #[serde(default)]
    pub games: HashMap<String, GameOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Default Discord Application Client ID (Shared Mode)
    #[serde(default = "default_client_id")]
    pub default_client_id: String,

    /// Interval in seconds between process table scans
    #[serde(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,

    /// Default details string template when in Shared Mode.
    /// Available variables: {game_title}
    #[serde(default = "default_shared_details")]
    pub shared_details_template: String,

    /// Default state string template.
    /// Available variables: {game_title}
    #[serde(default = "default_shared_state")]
    pub shared_state_template: String,

    /// Show elapsed play time in Rich Presence
    #[serde(default = "default_true")]
    pub show_elapsed_time: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiConfig {
    /// VNDB (The Visual Novel Database) API Token (from https://vndb.org/u/tokens)
    pub vndb_token: Option<String>,

    /// SteamGridDB API key (get one from https://www.steamgriddb.com/profile/preferences/api)
    pub steamgriddb_api_key: Option<String>,

    /// IGDB Client ID (Twitch Developer Portal)
    pub igdb_client_id: Option<String>,

    /// IGDB Client Secret (Twitch Developer Portal)
    pub igdb_client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameOverride {
    /// Friendly display name for the game (e.g. "Elden Ring: Shadow of the Erdtree")
    pub display_name: Option<String>,

    /// Custom Discord Application Client ID for Per-Game Mode.
    /// When set, the Discord status header will show "Playing <Discord App Name>"
    pub client_id: Option<String>,

    /// Direct HTTPS URL or asset key for the main game art
    pub image_url: Option<String>,

    /// Direct HTTPS URL or asset key for the small badge art
    pub small_image_url: Option<String>,

    /// Hover tooltip for the large image
    pub large_text: Option<String>,

    /// Hover tooltip for the small image
    pub small_text: Option<String>,

    /// Custom details line
    pub details: Option<String>,

    /// Custom state line
    pub state: Option<String>,

    /// Optional button 1 label
    pub button_1_label: Option<String>,

    /// Optional button 1 URL
    pub button_1_url: Option<String>,

    /// Optional button 2 label
    pub button_2_label: Option<String>,

    /// Optional button 2 URL
    pub button_2_url: Option<String>,

    /// If true, presence will never be published for this game
    #[serde(default)]
    pub ignore: bool,
}

fn default_client_id() -> String {
    // Default shared Client ID
    "1539094427459649686".to_string()
}

fn default_poll_interval_secs() -> u64 {
    3
}

fn default_shared_details() -> String {
    "{game_title}".to_string()
}

fn default_shared_state() -> String {
    "Reading / In-Game".to_string()
}

fn default_true() -> bool {
    true
}

fn default_ignored_processes() -> Vec<String> {
    vec![
        // System & Windows internals
        "system", "system idle process", "smss.exe", "csrss.exe", "wininit.exe",
        "services.exe", "lsass.exe", "svchost.exe", "fontdrvhost.exe", "dwm.exe",
        "explorer.exe", "sihost.exe", "taskhostw.exe", "ctfmon.exe", "conhost.exe",
        "runtimebroker.exe", "shellexperiencehost.exe", "searchhost.exe", "startmenuexperiencehost.exe",
        "lockapp.exe", "securityhealthsystray.exe", "securityhealthservice.exe",
        "wlanext.exe", "audiodg.exe", "spoolsv.exe", "wmiprvse.exe", "smartscreen.exe",
        "textinputhost.exe", "applicationframehost.exe", "compattelrunner.exe",
        // Browsers & Developer tools
        "chrome.exe", "firefox.exe", "msedge.exe", "brave.exe", "opera.exe", "vivaldi.exe",
        "code.exe", "cursor.exe", "idea64.exe", "pycharm64.exe", "rustrover64.exe",
        "cmd.exe", "powershell.exe", "pwsh.exe", "wt.exe", "windowsterminal.exe",
        "git.exe", "cargo.exe", "rustc.exe", "node.exe", "python.exe",
        // Communication & Media
        "discord.exe", "discordcanary.exe", "discordptb.exe", "slack.exe", "teams.exe",
        "spotify.exe", "vlc.exe", "obs64.exe", "obs32.exe", "telegram.exe",
        // Launchers & Overlays (not the game itself)
        "steam.exe", "steamservice.exe", "steamwebhelper.exe",
        "epicgameslauncher.exe", "unrealcefsubprocess.exe",
        "galaxyclient.exe", "galaxyclientservice.exe",
        "ea.exe", "eadesktop.exe", "eabackgroundservice.exe", "origin.exe",
        "battle.net.exe", "agent.exe", "riotclientservices.exe", "riotclientux.exe",
        "nvcontainer.exe", "geforceexperience.exe", "radeonsoftware.exe",
        "any-games-discord-rich-presence.exe",
    ]
    .into_iter()
    .map(|s| s.to_lowercase())
    .collect()
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_client_id: default_client_id(),
            poll_interval_secs: default_poll_interval_secs(),
            shared_details_template: default_shared_details(),
            shared_state_template: default_shared_state(),
            show_elapsed_time: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            api: ApiConfig::default(),
            ignored_processes: default_ignored_processes(),
            games: HashMap::new(),
        }
    }
}

impl AppConfig {
    /// Returns standard config directory path
    pub fn get_config_dir() -> PathBuf {
        if let Some(proj_dirs) = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION) {
            proj_dirs.config_dir().to_path_buf()
        } else {
            PathBuf::from("./config")
        }
    }

    /// Returns standard cache directory path (for SQLite DB and metadata cache)
    pub fn get_cache_dir() -> PathBuf {
        if let Some(proj_dirs) = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION) {
            proj_dirs.cache_dir().to_path_buf()
        } else {
            PathBuf::from("./cache")
        }
    }

    /// Default config file path
    pub fn get_config_file_path() -> PathBuf {
        Self::get_config_dir().join("config.toml")
    }

    /// Load config from standard location, creating a sample config if missing
    pub fn load_or_create(custom_path: Option<&Path>) -> Result<Self> {
        let config_path = match custom_path {
            Some(p) => p.to_path_buf(),
            None => Self::get_config_file_path(),
        };

        if !config_path.exists() {
            info!("Config file not found at {:?}. Generating default template...", config_path);
            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create config directory at {:?}", parent)
                })?;
            }
            let default_template = Self::generate_template();
            fs::write(&config_path, default_template).with_context(|| {
                format!("Failed to write default config to {:?}", config_path)
            })?;
            info!("Default configuration created at {:?}", config_path);
        }

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file at {:?}", config_path))?;

        let config: AppConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML configuration from {:?}", config_path))?;

        Ok(config)
    }

    /// Find an override for an executable name (checking both "game" and "game.exe")
    pub fn find_game_override(&self, exe_name: &str) -> Option<&GameOverride> {
        let lower = exe_name.to_lowercase();
        let stem = lower.trim_end_matches(".exe");

        self.games
            .get(&lower)
            .or_else(|| self.games.get(stem))
    }

    /// Check if a process is in the blacklist
    pub fn is_ignored(&self, exe_name: &str) -> bool {
        let lower = exe_name.to_lowercase();
        let stem = lower.trim_end_matches(".exe");

        if let Some(ov) = self.find_game_override(&lower) {
            if ov.ignore {
                return true;
            }
        }

        self.ignored_processes
            .iter()
            .any(|p| p.to_lowercase() == lower || p.to_lowercase().trim_end_matches(".exe") == stem)
    }

    /// Template content for a fresh config file
    pub fn generate_template() -> &'static str {
        r#"# Any Game Discord Rich Presence Daemon Configuration

[general]
# Default Discord Application Client ID used for Shared Mode (Zero Setup).
# In Shared Mode, Discord displays "Playing <App Name>", Details = "<Game Name>", Large Image = Cover Art.
# You can replace this with your own Discord Application Client ID created at:
# https://discord.com/developers/applications
default_client_id = "1539094427459649686"

# Polling frequency in seconds to check for active game processes (default: 3)
poll_interval_secs = 3

# Show elapsed playtime timer (default: true)
show_elapsed_time = true

# Template for Shared Mode details line ({game_title} will be replaced with detected title)
shared_details_template = "{game_title}"

# Template for Shared Mode state line
shared_state_template = "Reading / In-Game"

[api]
# VNDB (The Visual Novel Database) Token (from https://vndb.org/u/tokens)
vndb_token = ""

# SteamGridDB API Key (Recommended for general game cover art lookups)
# Get a free key at: https://www.steamgriddb.com/profile/preferences/api
steamgriddb_api_key = ""

# IGDB / Twitch API Credentials (Optional alternative metadata provider)
# Get credentials at: https://dev.twitch.tv/console/apps
igdb_client_id = ""
igdb_client_secret = ""

# List of process executables to ignore (case-insensitive)
ignored_processes = [
    "explorer.exe",
    "chrome.exe",
    "firefox.exe",
    "msedge.exe",
    "discord.exe",
    "spotify.exe",
    "code.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "epicgameslauncher.exe"
]

# ==============================================================================
# PER-GAME CONFIGURATION & OVERRIDES
# ==============================================================================
# You can define custom behavior for any game executable (with or without .exe).

# Example 1: Shared Mode with custom display name and image
# [games.eldenring]
# display_name = "Elden Ring: Shadow of the Erdtree"
# image_url = "https://images.igdb.com/igdb/image/upload/t_cover_big/co49x5.jpg"
# state = "Exploring the Lands Between"
# button_1_label = "Game Info"
# button_1_url = "https://en.bandainamcoent.eu/elden-ring/elden-ring"

# Example 2: Per-Game Mode (Power User)
# Set your own Discord Application Client ID so the header says "Playing Hollow Knight"
# [games.hollow_knight]
# client_id = "123456789012345678"
# display_name = "Hollow Knight"
# details = "Exploring Hallownest"
# state = "Steel Soul Mode"
# image_url = "https://cdn2.steamgriddb.com/grid/6cf6c986c753b879c3886f4a860b86a8.png"

# Example 3: Ignore a specific binary
# [games.benchmark_tool]
# ignore = true
"#
    }
}
