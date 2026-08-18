use crate::cache::MetadataCache;
use crate::config::{AppConfig, GameOverride};
use crate::discord_ipc::packet::{
    ActivityAssets, ActivityButton, ActivityPayload, ActivityTimestamps,
};
use crate::discord_ipc::DiscordIpcClient;
use crate::game_db::{GameMetadata, GameResolver};
use crate::process::{GameProcess, ProcessEvent, ProcessWatcher};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct DaemonStatus {
    pub is_paused: bool,
    #[allow(dead_code)]
    pub is_discord_connected: bool,
    pub current_game: Option<String>,
    #[allow(dead_code)]
    pub current_client_id: Option<String>,
    #[allow(dead_code)]
    pub active_mode: String,
}

pub struct RpcManager {
    config: Arc<RwLock<AppConfig>>,
    process_watcher: ProcessWatcher,
    game_resolver: GameResolver,
    discord_ipc: DiscordIpcClient,
    current_game_proc: Option<GameProcess>,
    is_paused: bool,
}

impl RpcManager {
    pub fn new(config: Arc<RwLock<AppConfig>>, cache: Arc<MetadataCache>) -> Self {
        let conf_guard = {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async { config.read().await.clone() })
        };

        let process_watcher = ProcessWatcher::new();
        let game_resolver = GameResolver::new(&conf_guard, cache);
        let discord_ipc = DiscordIpcClient::new();

        Self {
            config,
            process_watcher,
            game_resolver,
            discord_ipc,
            current_game_proc: None,
            is_paused: false,
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.is_paused = paused;
        if paused {
            info!("Daemon paused by user");
        } else {
            info!("Daemon resumed by user");
        }
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused
    }

    pub async fn update_config(&mut self, new_config: AppConfig, cache: Arc<MetadataCache>) {
        self.game_resolver = GameResolver::new(&new_config, cache);
        *self.config.write().await = new_config;
        info!("RPC Manager updated configuration");
    }

    /// Single tick of the manager loop
    pub async fn tick(&mut self) {
        if self.is_paused {
            return;
        }

        let config = self.config.read().await.clone();
        let event = self.process_watcher.poll(&config);

        if let Some(ev) = event {
            match ev {
                ProcessEvent::Started(proc) => {
                    self.on_game_started(proc, &config).await;
                }
                ProcessEvent::Stopped(proc) => {
                    self.on_game_stopped(proc).await;
                }
                ProcessEvent::Changed { current, .. } => {
                    if let Some(proc) = current {
                        self.on_game_started(proc, &config).await;
                    } else if let Some(prev) = self.current_game_proc.take() {
                        self.on_game_stopped(prev).await;
                    }
                }
            }
        }
    }

    async fn on_game_started(&mut self, proc: GameProcess, config: &AppConfig) {
        self.current_game_proc = Some(proc.clone());
        let game_ov = config.find_game_override(&proc.exe_name);

        // Determine Client ID
        // Per-Game Mode: override specifies custom client_id
        // Shared Mode: fallback to general.default_client_id
        let (target_client_id, is_per_game) = match game_ov.and_then(|ov| ov.client_id.as_ref()) {
            Some(cid) if !cid.trim().is_empty() => (cid.clone(), true),
            _ => (config.general.default_client_id.clone(), false),
        };

        // Resolve Metadata using exe name and parent folder
        let metadata = match self
            .game_resolver
            .resolve(
                &proc.exe_name,
                &proc.clean_name,
                proc.folder_name.as_deref(),
                config,
            )
            .await
        {
            Ok(meta) => meta,
            Err(e) => {
                warn!("Metadata resolution error: {:#}", e);
                GameMetadata {
                    display_name: proc.clean_name.clone(),
                    image_url: None,
                    small_image_url: None,
                    source: "fallback".to_string(),
                }
            }
        };

        info!(
            "Publishing Rich Presence for '{}' [Mode: {}] (Client ID: {})",
            metadata.display_name,
            if is_per_game { "Per-Game" } else { "Shared" },
            target_client_id
        );

        // Connect to Discord IPC
        if let Err(e) = self.discord_ipc.ensure_connected(&target_client_id).await {
            warn!("Could not connect to Discord IPC: {:#}. Will retry next cycle.", e);
            return;
        }

        // Build Activity Payload
        let activity = Self::build_activity_payload(&proc, &metadata, game_ov, config, is_per_game);

        if let Err(e) = self.discord_ipc.set_activity(proc.pid, activity).await {
            warn!("Failed to push Rich Presence to Discord: {:#}", e);
        }
    }

    async fn on_game_stopped(&mut self, proc: GameProcess) {
        info!("Clearing Rich Presence for PID {}", proc.pid);
        let _ = self.discord_ipc.clear_activity(proc.pid).await;
        self.current_game_proc = None;
    }

    fn build_activity_payload(
        proc: &GameProcess,
        metadata: &GameMetadata,
        game_ov: Option<&GameOverride>,
        config: &AppConfig,
        is_per_game: bool,
    ) -> ActivityPayload {
        let (details, state) = if is_per_game {
            // Per-Game Mode: Discord Header says "Playing <App Name>"
            let d = game_ov.and_then(|ov| ov.details.clone());
            let s = game_ov
                .and_then(|ov| ov.state.clone())
                .or_else(|| Some("Reading / In-Game".to_string()));
            (d, s)
        } else {
            // Shared Mode: Details shows real Game Title
            let d = game_ov
                .and_then(|ov| ov.details.clone())
                .unwrap_or_else(|| {
                    config
                        .general
                        .shared_details_template
                        .replace("{game_title}", &metadata.display_name)
                });
            let s = game_ov
                .and_then(|ov| ov.state.clone())
                .unwrap_or_else(|| {
                    config
                        .general
                        .shared_state_template
                        .replace("{game_title}", &metadata.display_name)
                });
            (Some(d), Some(s))
        };

        let timestamps = if config.general.show_elapsed_time {
            Some(ActivityTimestamps {
                start: Some(proc.start_time),
                end: None,
            })
        } else {
            None
        };

        let large_image = game_ov
            .and_then(|ov| ov.image_url.clone())
            .or_else(|| metadata.image_url.clone());

        let small_image = game_ov
            .and_then(|ov| ov.small_image_url.clone())
            .or_else(|| metadata.small_image_url.clone());

        let large_text = game_ov
            .and_then(|ov| ov.large_text.clone())
            .or_else(|| Some(metadata.display_name.clone()));

        let small_text = game_ov.and_then(|ov| ov.small_text.clone());

        let assets = if large_image.is_some() || small_image.is_some() {
            Some(ActivityAssets {
                large_image,
                large_text,
                small_image,
                small_text,
            })
        } else {
            None
        };

        // Buttons (optional)
        let mut buttons = Vec::new();
        if let Some(ov) = game_ov {
            if let (Some(ref l1), Some(ref u1)) = (&ov.button_1_label, &ov.button_1_url) {
                buttons.push(ActivityButton {
                    label: l1.clone(),
                    url: u1.clone(),
                });
            }
            if let (Some(ref l2), Some(ref u2)) = (&ov.button_2_label, &ov.button_2_url) {
                buttons.push(ActivityButton {
                    label: l2.clone(),
                    url: u2.clone(),
                });
            }
        }

        let buttons_opt = if !buttons.is_empty() {
            Some(buttons)
        } else {
            None
        };

        ActivityPayload {
            details,
            state,
            timestamps,
            assets,
            buttons: buttons_opt,
            instance: Some(false),
        }
    }

    pub fn get_status(&self) -> DaemonStatus {
        let is_connected = self.discord_ipc.is_connected();
        let client_id = self.discord_ipc.active_client_id().map(|s| s.to_string());
        let game_name = self.current_game_proc.as_ref().map(|p| p.clean_name.clone());

        let active_mode = match (self.current_game_proc.is_some(), &client_id) {
            (true, Some(cid)) => {
                let conf = {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async { self.config.read().await.clone() })
                };
                if cid == &conf.general.default_client_id {
                    "Shared Mode".to_string()
                } else {
                    "Per-Game Mode".to_string()
                }
            }
            _ => "Idle".to_string(),
        };

        DaemonStatus {
            is_paused: self.is_paused,
            is_discord_connected: is_connected,
            current_game: game_name,
            current_client_id: client_id,
            active_mode,
        }
    }

    pub async fn shutdown(&mut self) {
        if let Some(proc) = self.current_game_proc.take() {
            let _ = self.discord_ipc.clear_activity(proc.pid).await;
        }
        self.discord_ipc.disconnect().await;
        info!("RPC Manager shut down cleanly");
    }
}
