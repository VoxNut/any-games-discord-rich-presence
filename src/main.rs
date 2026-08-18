mod cache;
mod config;
mod discord_ipc;
mod game_db;
mod manager;
mod process;
mod tray;

use anyhow::{Context, Result};
use cache::MetadataCache;
use clap::Parser;
use config::AppConfig;
use discord_ipc::packet::{ActivityAssets, ActivityPayload, ActivityTimestamps};
use discord_ipc::DiscordIpcClient;
use game_db::GameResolver;
use manager::RpcManager;
use process::ProcessWatcher;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use tray::{TrayCommand, TrayManager};

#[derive(Parser, Debug)]
#[command(
    name = "any-games-discord-rich-presence",
    author = "VoxNut",
    version = "0.1.0",
    about = "Lightweight daemon providing Discord Rich Presence for any game"
)]
struct Cli {
    /// Path to custom config file (default: OS config directory)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Run in headless mode without system tray icon
    #[arg(long)]
    headless: bool,

    /// Test process detection and list running game candidates, then exit
    #[arg(long)]
    test_scan: bool,

    /// Test metadata resolution for a game title / executable name, then exit
    #[arg(long)]
    test_lookup: Option<String>,

    /// Send an immediate test Rich Presence activity to Discord, wait 10s, and exit
    #[arg(long)]
    test_presence: Option<String>,

    /// Custom Client ID for --test-presence
    #[arg(long)]
    client_id: Option<String>,

    /// Custom cover art image URL for --test-presence
    #[arg(long)]
    cover_url: Option<String>,

    /// Generate default config file if missing and print path, then exit
    #[arg(long)]
    init_config: bool,

    /// Clear local SQLite metadata cache, then exit
    #[arg(long)]
    clear_cache: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    // 1. Diagnostics / One-off CLI tasks
    if cli.init_config {
        let path = AppConfig::get_config_file_path();
        let _ = AppConfig::load_or_create(cli.config.as_deref())?;
        println!("Configuration file initialized at: {}", path.display());
        return Ok(());
    }

    let cache_dir = AppConfig::get_cache_dir();
    let cache = Arc::new(MetadataCache::new(cache_dir.clone())?);

    if cli.clear_cache {
        cache.clear()?;
        println!("Metadata cache cleared.");
        return Ok(());
    }

    let app_config = AppConfig::load_or_create(cli.config.as_deref())?;

    if cli.test_scan {
        println!("Scanning running processes for game candidates...\n");
        let mut watcher = ProcessWatcher::new();
        let candidates = watcher.scan_all_candidates(&app_config);

        if candidates.is_empty() {
            println!("No running game candidates detected.");
            println!("(Tip: If your game is running, check if it's in 'ignored_processes' in config.toml)");
        } else {
            println!("{:<8} {:<30} {:<30} {:<30}", "PID", "EXECUTABLE", "CLEAN NAME", "PATH");
            println!("{:-<100}", "");
            for c in candidates {
                let path_str = c
                    .exe_path
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "N/A".to_string());
                println!("{:<8} {:<30} {:<30} {:<30}", c.pid, c.exe_name, c.clean_name, path_str);
            }
        }
        return Ok(());
    }

    if let Some(ref title) = cli.test_lookup {
        println!("Testing metadata resolution for: '{}'...\n", title);
        let resolver = GameResolver::new(&app_config, cache.clone());
        let clean = process::clean_executable_name(title);
        let metadata = resolver.resolve(title, &clean, &app_config).await?;

        println!("Resolved Game Metadata:");
        println!("  Display Name: {}", metadata.display_name);
        println!("  Source:       {}", metadata.source);
        println!("  Image URL:    {}", metadata.image_url.as_deref().unwrap_or("None"));
        println!("  Small Image:  {}", metadata.small_image_url.as_deref().unwrap_or("None"));
        return Ok(());
    }

    if let Some(ref title) = cli.test_presence {
        let client_id = cli
            .client_id
            .unwrap_or_else(|| app_config.general.default_client_id.clone());

        println!(
            "Sending test Rich Presence for '{}' using Client ID '{}'...",
            title, client_id
        );

        let mut ipc = DiscordIpcClient::new();
        ipc.ensure_connected(&client_id).await.context(
            "Failed to connect to Discord IPC. Please make sure Discord is running!",
        )?;

        let activity = ActivityPayload {
            details: Some(title.clone()),
            state: Some("Testing Generic RPC Daemon".to_string()),
            timestamps: Some(ActivityTimestamps {
                start: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                ),
                end: None,
            }),
            assets: Some(ActivityAssets {
                large_image: cli.cover_url.or_else(|| {
                    Some("https://cdn2.steamgriddb.com/grid/6cf6c986c753b879c3886f4a860b86a8.png".to_string())
                }),
                large_text: Some(title.clone()),
                small_image: None,
                small_text: None,
            }),
            buttons: None,
            instance: Some(false),
        };

        let current_pid = std::process::id();
        ipc.set_activity(current_pid, activity).await?;
        println!("Presence updated successfully! Check your Discord profile.");
        println!("Waiting 10 seconds before clearing presence...");

        tokio::time::sleep(Duration::from_secs(10)).await;
        ipc.clear_activity(current_pid).await?;
        ipc.disconnect().await;
        println!("Test completed.");
        return Ok(());
    }

    // 2. Normal Daemon Mode
    info!("Starting Any Game Discord Rich Presence Daemon v0.1.0");
    info!("Config path: {:?}", AppConfig::get_config_file_path());
    info!("Cache directory: {:?}", cache_dir);

    let config_arc = Arc::new(RwLock::new(app_config.clone()));
    let mut manager = RpcManager::new(config_arc.clone(), cache.clone());

    if cli.headless {
        info!("Running in Headless Mode (no tray icon)");
        run_headless_loop(&mut manager, &app_config).await?;
    } else {
        info!("Running with System Tray Icon");
        run_tray_loop(&mut manager, config_arc, cache, cli.config.as_deref()).await?;
    }

    Ok(())
}

async fn run_headless_loop(manager: &mut RpcManager, config: &AppConfig) -> Result<()> {
    let poll_interval = Duration::from_secs(config.general.poll_interval_secs);
    let mut ticker = tokio::time::interval(poll_interval);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl+C received. Shutting down...");
                manager.shutdown().await;
                break;
            }
            _ = ticker.tick() => {
                manager.tick().await;
            }
        }
    }

    Ok(())
}

async fn run_tray_loop(
    manager: &mut RpcManager,
    config_arc: Arc<RwLock<AppConfig>>,
    cache: Arc<MetadataCache>,
    custom_config_path: Option<&std::path::Path>,
) -> Result<()> {
    let tray_manager = match TrayManager::new() {
        Ok(tm) => tm,
        Err(e) => {
            warn!("Failed to create system tray icon: {:#}. Falling back to headless mode.", e);
            let conf = config_arc.read().await.clone();
            return run_headless_loop(manager, &conf).await;
        }
    };

    let poll_interval_secs = {
        let conf = config_arc.read().await;
        conf.general.poll_interval_secs
    };

    let mut ticker = tokio::time::interval(Duration::from_secs(poll_interval_secs));
    let mut tray_event_ticker = tokio::time::interval(Duration::from_millis(100));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Ctrl+C received. Shutting down...");
                manager.shutdown().await;
                break;
            }
            _ = tray_event_ticker.tick() => {
                if let Some(cmd) = tray_manager.handle_menu_events() {
                    match cmd {
                        TrayCommand::TogglePause => {
                            let new_state = !manager.is_paused();
                            manager.set_paused(new_state);
                            let status = manager.get_status();
                            tray_manager.update_status(status.is_paused, status.current_game.as_deref());
                        }
                        TrayCommand::ReloadConfig => {
                            info!("Reloading configuration from disk...");
                            match AppConfig::load_or_create(custom_config_path) {
                                Ok(new_cfg) => {
                                    manager.update_config(new_cfg, cache.clone()).await;
                                    info!("Configuration reloaded successfully!");
                                }
                                Err(e) => {
                                    error!("Failed to reload config: {:#}", e);
                                }
                            }
                        }
                        TrayCommand::OpenConfigDir => {
                            let dir = AppConfig::get_config_dir();
                            info!("Opening config folder: {:?}", dir);
                            let _ = open::that(&dir);
                        }
                        TrayCommand::ClearCache => {
                            let _ = cache.clear();
                        }
                        TrayCommand::Quit => {
                            info!("Quit requested via tray menu.");
                            manager.shutdown().await;
                            break;
                        }
                    }
                }
            }
            _ = ticker.tick() => {
                manager.tick().await;
                let status = manager.get_status();
                tray_manager.update_status(status.is_paused, status.current_game.as_deref());
            }
        }
    }

    Ok(())
}
