# Any Game Discord Rich Presence Daemon (Rust)

A high-performance, lightweight Rust desktop daemon that automatically displays **Discord Rich Presence for any game**, including games Discord has no official integration for.

Runs quietly in the background, watches running processes, queries game metadata & high-resolution cover art (SteamGridDB / IGDB), caches results locally in embedded SQLite, and streams presence updates via Discord's local IPC named pipe / Unix domain socket.

---

## Features

- 🎮 **Universal Game Detection**: Automatically tracks launched games using low-CPU process polling (`sysinfo`) and smart binary name sanitization.
- 🎨 **Live Remote Cover Art**: Uses direct HTTPS image URLs (`assets.large_image`) from **SteamGridDB** and **IGDB** without pre-uploading assets to Discord Developer Portal.
- ⚡ **Dual Client ID Modes**:
  - **Shared Mode (Default / Zero Setup)**: Uses a shared Discord Application ID. Displays the game title in `details`, live metadata/custom text in `state`, and high-res cover art.
  - **Per-Game Mode (Power User)**: Register a Discord Application per game and configure its Client ID in `config.toml` so the header displays `"Playing <Game Name>"`. The daemon automatically swaps Client IDs upon game launch.
- 💾 **Local SQLite Cache**: Metadata is stored in a zero-dependency embedded SQLite database (`rusqlite` bundled) to avoid duplicate API calls and respect rate limits.
- 🎛️ **Manual Overrides**: TOML config file allowing custom titles, image URLs, state/details text, ignore flags, and clickable action buttons.
- 🖥️ **System Tray & Headless Support**: Cross-platform system tray icon (`tray-icon`) with pause/resume, status display, config reloading, and directory shortcuts, or standalone `--headless` CLI mode.
- 🔒 **Zero Telemetry & Private**: No analytics or bundled credentials. All API keys and IDs live in your local user directory.

---

## Installation & Build

### Prerequisites
- [Rust & Cargo](https://www.rust-lang.org/tools/install) (1.75+)
- Discord Desktop client running locally

### Clone & Build
```bash
git clone https://github.com/VoxNut/any-games-discord-rich-presence.git
cd any-games-discord-rich-presence
cargo build --release
```
The compiled binary will be located at `target/release/any-games-discord-rich-presence.exe`.

---

## Configuration & First-Run Setup

Generate your configuration file and print its location:
```bash
cargo run -- --init-config
```

### Configuration Locations
- **Windows**: `%APPDATA%\voxnut\any-games-discord-rich-presence\config\config.toml`
- **Linux**: `~/.config/any-games-discord-rich-presence/config.toml`
- **macOS**: `~/Library/Application Support/voxnut/any-games-discord-rich-presence/config.toml`

---

## Obtaining API Keys & Client IDs

### 1. Discord Application Client ID
Discord Rich Presence requires a **Client ID** from the Discord Developer Portal:
1. Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2. Click **New Application**.
3. Give your application a name:
   - For **Shared Mode**, name it something generic like `Any Game RPC` or `Game Launcher`.
   - For **Per-Game Mode**, name it the exact name of the game (e.g. `Hollow Knight` or `Elden Ring`).
4. Copy the **Application ID** (Client ID) from the **General Information** page.
5. Paste it into `config.toml` under `general.default_client_id` or your game override `[games.<name>].client_id`.

### 2. SteamGridDB API Key (Recommended)
SteamGridDB provides free access to high-quality cover art grids and game icons:
1. Sign in to [SteamGridDB](https://www.steamgriddb.com/).
2. Navigate to your [Preferences -> API](https://www.steamgriddb.com/profile/preferences/api).
3. Generate an API Key.
4. Add it to `config.toml` under `[api].steamgriddb_api_key`.

### 3. IGDB API Credentials (Optional)
1. Log in to the [Twitch Developer Console](https://dev.twitch.tv/console/apps).
2. Register a new application (Category: *Application Integration*, OAuth Redirect URL: `http://localhost`).
3. Copy the **Client ID** and generate a **Client Secret**.
4. Add them to `config.toml` under `[api].igdb_client_id` and `[api].igdb_client_secret`.

---

## Example `config.toml`

```toml
[general]
default_client_id = "1340987654321098765"
poll_interval_secs = 3
show_elapsed_time = true
shared_details_template = "{game_title}"
shared_state_template = "In-Game"

[api]
steamgriddb_api_key = "your_steamgriddb_api_key_here"
igdb_client_id = ""
igdb_client_secret = ""

ignored_processes = [
    "explorer.exe",
    "chrome.exe",
    "firefox.exe",
    "discord.exe",
    "spotify.exe",
    "steam.exe",
    "epicgameslauncher.exe"
]

# Shared Mode override with custom image & clickable buttons
[games.eldenring]
display_name = "Elden Ring: Shadow of the Erdtree"
image_url = "https://images.igdb.com/igdb/image/upload/t_cover_big/co49x5.jpg"
state = "Exploring the Lands Between"
button_1_label = "Game Info"
button_1_url = "https://en.bandainamcoent.eu/elden-ring/elden-ring"

# Per-Game Mode: Discord status header displays "Playing Hollow Knight"
[games.hollow_knight]
client_id = "123456789012345678"
display_name = "Hollow Knight"
details = "Hallownest"
state = "Steel Soul Mode"
image_url = "https://cdn2.steamgriddb.com/grid/6cf6c986c753b879c3886f4a860b86a8.png"
```

---

## CLI Options & Diagnostics

```text
Usage: any-games-discord-rich-presence [OPTIONS]

Options:
  -c, --config <CONFIG>          Path to custom config file
      --headless                 Run in headless mode without system tray icon
      --test-scan                Test process detection and list running game candidates
      --test-lookup <GAME>       Test metadata resolution for a game title / executable
      --test-presence <GAME>     Send an immediate test Rich Presence activity to Discord
      --client-id <CLIENT_ID>    Custom Client ID for --test-presence
      --cover-url <URL>          Custom cover art image URL for --test-presence
      --init-config              Generate default config file if missing and print path
      --clear-cache              Clear local SQLite metadata cache
  -h, --help                     Print help
  -V, --version                  Print version
```

### Diagnostic Examples
- Scan running game processes:
  ```bash
  cargo run -- --test-scan
  ```
- Test cover art lookup for a game:
  ```bash
  cargo run -- --test-lookup "witcher3.exe"
  ```
- Send a 10-second test presence to Discord:
  ```bash
  cargo run -- --test-presence "Hollow Knight"
  ```

---

## Troubleshooting

### Discord Not Detected / Connection Dropped
- Ensure Discord Desktop is running locally before or during daemon operation.
- On Linux, if using Flatpak or Snap, ensure the daemon has access to `$XDG_RUNTIME_DIR/app/com.discordapp.Discord` or `/tmp`.

### Cover Art Not Showing
- Verify your SteamGridDB API key in `config.toml`.
- Discord supports direct HTTPS URLs (`png`, `jpg`, `webp`, `gif`). Make sure URLs are publicly reachable HTTPS endpoints.
- If using manual `image_url` overrides, ensure the URL begins with `https://`.

### Executable Not Recognized
- Run `--test-scan` while the game is running to see if the process is detected.
- If the game is ignored, check the `ignored_processes` list in `config.toml`.
- You can add an explicit entry under `[games.<executable_name>]` in `config.toml`.

---

## License

MIT License. See [LICENSE](LICENSE) for details.
