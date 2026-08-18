use anyhow::{Context, Result};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub enum TrayCommand {
    TogglePause,
    ReloadConfig,
    OpenConfigDir,
    ClearCache,
    Quit,
}

pub struct TrayManager {
    _tray_icon: TrayIcon,
    menu_status: MenuItem,
    menu_pause: MenuItem,
    menu_reload: MenuItem,
    menu_open_config: MenuItem,
    menu_clear_cache: MenuItem,
    menu_quit: MenuItem,
}

impl TrayManager {
    pub fn new() -> Result<Self> {
        let tray_menu = Menu::new();

        let menu_status = MenuItem::new("Status: Idle", false, None);
        let menu_pause = MenuItem::new("Pause Watching", true, None);
        let menu_reload = MenuItem::new("Reload Config", true, None);
        let menu_open_config = MenuItem::new("Open Config Folder", true, None);
        let menu_clear_cache = MenuItem::new("Clear Metadata Cache", true, None);
        let menu_quit = MenuItem::new("Quit", true, None);

        let sep1 = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();

        tray_menu.append(&menu_status)?;
        tray_menu.append(&sep1)?;
        tray_menu.append(&menu_pause)?;
        tray_menu.append(&menu_reload)?;
        tray_menu.append(&menu_open_config)?;
        tray_menu.append(&menu_clear_cache)?;
        tray_menu.append(&sep2)?;
        tray_menu.append(&menu_quit)?;

        let icon = create_gamepad_icon();

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("Any Game Discord Rich Presence")
            .with_icon(icon)
            .build()
            .context("Failed to build system tray icon")?;

        Ok(Self {
            _tray_icon: tray_icon,
            menu_status,
            menu_pause,
            menu_reload,
            menu_open_config,
            menu_clear_cache,
            menu_quit,
        })
    }

    /// Update status line in the tray menu
    pub fn update_status(&self, is_paused: bool, current_game: Option<&str>) {
        let text = if is_paused {
            "Status: Paused".to_string()
        } else if let Some(game) = current_game {
            format!("Playing: {}", game)
        } else {
            "Status: Watching for Games".to_string()
        };

        self.menu_status.set_text(text);
        self.menu_pause.set_text(if is_paused {
            "Resume Watching"
        } else {
            "Pause Watching"
        });
    }

    /// Check for menu click events
    pub fn handle_menu_events(&self) -> Option<TrayCommand> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.menu_pause.id() {
                return Some(TrayCommand::TogglePause);
            } else if event.id == self.menu_reload.id() {
                return Some(TrayCommand::ReloadConfig);
            } else if event.id == self.menu_open_config.id() {
                return Some(TrayCommand::OpenConfigDir);
            } else if event.id == self.menu_clear_cache.id() {
                return Some(TrayCommand::ClearCache);
            } else if event.id == self.menu_quit.id() {
                return Some(TrayCommand::Quit);
            }
        }
        None
    }
}

/// Generates a 32x32 RGBA gamepad icon (Discord Blurple #5865F2)
fn create_gamepad_icon() -> Icon {
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 32;
    let mut rgba = vec![0u8; (WIDTH * HEIGHT * 4) as usize];

    // Discord Blurple RGBA
    let blurple = [88, 101, 242, 255];
    let white = [255, 255, 255, 255];
    let dark_blurple = [50, 60, 160, 255];

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let idx = ((y * WIDTH + x) * 4) as usize;

            // Draw controller body: rounded rect between (4, 8) and (27, 24)
            let in_body = x >= 4 && x <= 27 && y >= 8 && y <= 24;
            let in_left_grip = x >= 2 && x <= 10 && y >= 14 && y <= 28;
            let in_right_grip = x >= 21 && x <= 29 && y >= 14 && y <= 28;

            if in_body || in_left_grip || in_right_grip {
                // Outer controller shape
                rgba[idx..idx + 4].copy_from_slice(&blurple);

                // D-Pad on left (x: 7..11, y: 13..17)
                let is_dpad_v = x == 9 && y >= 13 && y <= 17;
                let is_dpad_h = y == 15 && x >= 7 && x <= 11;
                if is_dpad_v || is_dpad_h {
                    rgba[idx..idx + 4].copy_from_slice(&white);
                }

                // Action buttons on right (x: 21..25, y: 13..17)
                let is_btn_top = x == 23 && y == 13;
                let is_btn_bot = x == 23 && y == 17;
                let is_btn_l = x == 21 && y == 15;
                let is_btn_r = x == 25 && y == 15;
                if is_btn_top || is_btn_bot || is_btn_l || is_btn_r {
                    rgba[idx..idx + 4].copy_from_slice(&white);
                }

                // Center home button
                if (x == 15 || x == 16) && (y == 14 || y == 15) {
                    rgba[idx..idx + 4].copy_from_slice(&dark_blurple);
                }
            }
        }
    }

    Icon::from_rgba(rgba, WIDTH, HEIGHT).expect("Failed to create icon from RGBA buffer")
}
