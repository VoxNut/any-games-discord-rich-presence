use crate::config::AppConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tracing::info;

/// Represents an identified running game process
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProcess {
    pub pid: u32,
    pub exe_name: String,
    pub clean_name: String,
    pub exe_path: Option<PathBuf>,
    pub start_time: u64,
}

/// Events emitted by the process watcher
#[derive(Debug, Clone)]
pub enum ProcessEvent {
    Started(GameProcess),
    Stopped(GameProcess),
    Changed {
        #[allow(dead_code)]
        previous: Option<GameProcess>,
        current: Option<GameProcess>,
    },
}

pub struct ProcessWatcher {
    system: System,
    /// Currently tracked active game (if any)
    active_game: Option<GameProcess>,
    /// Previous scan's list of running game candidates (pid -> GameProcess)
    running_candidates: HashMap<u32, GameProcess>,
}

impl ProcessWatcher {
    pub fn new() -> Self {
        let mut system = System::new();
        // Initial process scan
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
        );

        Self {
            system,
            active_game: None,
            running_candidates: HashMap::new(),
        }
    }

    /// Scan system processes and return any change in the active target game
    pub fn poll(&mut self, config: &AppConfig) -> Option<ProcessEvent> {
        // Efficient lightweight refresh of process list
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::OnlyIfNotSet),
        );

        let mut current_candidates: HashMap<u32, GameProcess> = HashMap::new();

        for (pid, process) in self.system.processes() {
            let pid_u32 = pid.as_u32();
            let raw_name = process.name().to_string_lossy().to_string();
            let exe_path = process.exe().map(|p| p.to_path_buf());

            if config.is_ignored(&raw_name) {
                continue;
            }

            // Check if full path or folder name indicates an ignored system process
            if let Some(ref path) = exe_path {
                let path_str = path.to_string_lossy().to_lowercase();
                if is_system_path(&path_str) {
                    continue;
                }
            }

            // Check if explicitly configured or is a recognized game candidate
            let clean = clean_executable_name(&raw_name);
            if clean.is_empty() {
                continue;
            }

            let start_time = if process.start_time() > 0 {
                process.start_time()
            } else {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            };

            // If explicitly configured in config.games (and not ignored), or candidate
            let is_explicit = config.find_game_override(&raw_name).is_some();
            let is_heuristic_candidate = is_likely_game(&raw_name, exe_path.as_deref());

            if is_explicit || is_heuristic_candidate {
                current_candidates.insert(
                    pid_u32,
                    GameProcess {
                        pid: pid_u32,
                        exe_name: raw_name,
                        clean_name: clean,
                        exe_path,
                        start_time,
                    },
                );
            }
        }

        // Determine target game: prioritize explicitly configured overrides, then newest launched candidate
        let best_candidate = select_best_game(&current_candidates, config);

        let event = if self.active_game != best_candidate {
            let prev = self.active_game.clone();
            self.active_game = best_candidate.clone();

            match (&prev, &best_candidate) {
                (None, Some(curr)) => {
                    info!("Game detected: {} (PID: {})", curr.exe_name, curr.pid);
                    Some(ProcessEvent::Started(curr.clone()))
                }
                (Some(p), None) => {
                    info!("Game closed: {} (PID: {})", p.exe_name, p.pid);
                    Some(ProcessEvent::Stopped(p.clone()))
                }
                (Some(p), Some(curr)) => {
                    info!(
                        "Switched active game from {} (PID: {}) to {} (PID: {})",
                        p.exe_name, p.pid, curr.exe_name, curr.pid
                    );
                    Some(ProcessEvent::Changed {
                        previous: Some(p.clone()),
                        current: Some(curr.clone()),
                    })
                }
                (None, None) => None,
            }
        } else {
            None
        };

        self.running_candidates = current_candidates;
        event
    }

    /// List all currently running candidates (for CLI diagnostics)
    pub fn scan_all_candidates(&mut self, config: &AppConfig) -> Vec<GameProcess> {
        self.poll(config);
        let mut list: Vec<GameProcess> = self.running_candidates.values().cloned().collect();
        list.sort_by(|a, b| a.clean_name.cmp(&b.clean_name));
        list
    }

    #[allow(dead_code)]
    pub fn current_active_game(&self) -> Option<&GameProcess> {
        self.active_game.as_ref()
    }
}

/// Clean and normalize executable name (stripping `.exe`, `_Shipping`, etc.)
pub fn clean_executable_name(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let name = lower.trim_end_matches(".exe");

    // Remove common suffixes attached by engines (Unreal, Unity, etc.)
    let stripped = name
        .trim_end_matches("-win64-shipping")
        .trim_end_matches("_win64_shipping")
        .trim_end_matches("-shipping")
        .trim_end_matches("_shipping")
        .trim_end_matches("-x64")
        .trim_end_matches("_x64")
        .trim_end_matches("-x86")
        .trim_end_matches("_x86")
        .trim_end_matches("_dx11")
        .trim_end_matches("_dx12")
        .trim_end_matches("-dx11")
        .trim_end_matches("-dx12")
        .trim_end_matches("_steam")
        .trim_end_matches("-steam");

    stripped.to_string()
}

/// Heuristically determine if a process is likely a game
fn is_likely_game(exe_name: &str, exe_path: Option<&Path>) -> bool {
    let lower = exe_name.to_lowercase();

    // Check path for common game libraries or directories
    if let Some(path) = exe_path {
        let p = path.to_string_lossy().to_lowercase();
        if p.contains("steamapps\\common")
            || p.contains("steamapps/common")
            || p.contains("epic games")
            || p.contains("gog games")
            || p.contains("ubisoft game launcher\\games")
            || p.contains("ea games")
            || p.contains("xboxgames")
            || p.contains("riot games")
            || p.contains("\\games\\")
            || p.contains("/games/")
        {
            return true;
        }
    }

    // Engine patterns
    if lower.ends_with("-win64-shipping.exe")
        || lower.ends_with("_shipping.exe")
        || lower.contains("unitycrashhandler")
    {
        return !lower.contains("crashhandler");
    }

    false
}

/// Check if path is in Windows System / internal directory
fn is_system_path(path_str: &str) -> bool {
    path_str.contains("c:\\windows\\system32")
        || path_str.contains("c:\\windows\\syswow64")
        || path_str.contains("c:\\windows\\winsxs")
        || path_str.contains("c:\\windows\\systemapps")
        || path_str.contains("/usr/lib")
        || path_str.contains("/usr/bin")
        || path_str.contains("/system/")
}

/// Select best candidate: priority to explicit configs, then most recent start time
fn select_best_game(
    candidates: &HashMap<u32, GameProcess>,
    config: &AppConfig,
) -> Option<GameProcess> {
    if candidates.is_empty() {
        return None;
    }

    // 1. First look for candidates that have an explicit configuration in config.toml
    let explicit: Vec<&GameProcess> = candidates
        .values()
        .filter(|p| config.find_game_override(&p.exe_name).is_some())
        .collect();

    if let Some(best_explicit) = explicit.into_iter().max_by_key(|p| p.start_time) {
        return Some(best_explicit.clone());
    }

    // 2. Otherwise, pick the most recently launched candidate
    candidates
        .values()
        .max_by_key(|p| p.start_time)
        .cloned()
}
