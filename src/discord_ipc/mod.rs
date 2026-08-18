pub mod connection;
pub mod packet;

use anyhow::{Context, Result};
use connection::IpcConnection;
use packet::{ActivityPayload, Opcode, SetActivityArgs, SetActivityCommand};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

pub struct DiscordIpcClient {
    connection: Option<IpcConnection>,
    active_client_id: Option<String>,
}

impl DiscordIpcClient {
    pub fn new() -> Self {
        Self {
            connection: None,
            active_client_id: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }

    pub fn active_client_id(&self) -> Option<&str> {
        self.active_client_id.as_deref()
    }

    /// Ensure connected to Discord IPC with the specified Client ID.
    /// If already connected with the same Client ID, does nothing.
    /// If connected with a different Client ID, cleanly disconnects and reconnects.
    pub async fn ensure_connected(&mut self, client_id: &str) -> Result<()> {
        if let Some(current_id) = &self.active_client_id {
            if current_id == client_id && self.connection.is_some() {
                return Ok(());
            }
        }

        // Need to connect or switch client_id
        self.disconnect().await;

        debug!("Connecting to Discord IPC with Client ID '{}'...", client_id);
        match IpcConnection::connect(client_id).await {
            Ok(conn) => {
                info!("Connected to Discord IPC (Client ID: {})", client_id);
                self.connection = Some(conn);
                self.active_client_id = Some(client_id.to_string());
                Ok(())
            }
            Err(e) => {
                self.connection = None;
                self.active_client_id = None;
                Err(e)
            }
        }
    }

    /// Update Rich Presence activity
    pub async fn set_activity(&mut self, pid: u32, activity: ActivityPayload) -> Result<()> {
        let nonce = format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let command = SetActivityCommand {
            cmd: "SET_ACTIVITY".to_string(),
            args: SetActivityArgs {
                pid,
                activity: Some(activity),
            },
            nonce,
        };

        let json = serde_json::to_string(&command)?;
        if let Some(conn) = &mut self.connection {
            conn.send_packet(Opcode::Frame, &json)
                .await
                .context("Failed to send SET_ACTIVITY command")?;

            // Read response frame
            match conn.recv_packet().await {
                Ok((opcode, resp)) => {
                    debug!("SET_ACTIVITY response ({:?}): {}", opcode, resp);
                }
                Err(e) => {
                    warn!("Failed reading SET_ACTIVITY response from Discord: {:#}", e);
                }
            }
            Ok(())
        } else {
            anyhow::bail!("Discord IPC is not connected");
        }
    }

    /// Clear presence
    pub async fn clear_activity(&mut self, pid: u32) -> Result<()> {
        let nonce = format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );

        let command = SetActivityCommand {
            cmd: "SET_ACTIVITY".to_string(),
            args: SetActivityArgs { pid, activity: None },
            nonce,
        };

        let json = serde_json::to_string(&command)?;
        if let Some(conn) = &mut self.connection {
            let _ = conn.send_packet(Opcode::Frame, &json).await;
            debug!("Cleared Discord presence");
        }
        Ok(())
    }

    /// Graceful disconnect
    pub async fn disconnect(&mut self) {
        if let Some(mut conn) = self.connection.take() {
            debug!("Closing Discord IPC connection (Client ID: {:?})", self.active_client_id);
            let _ = conn.close().await;
        }
        self.active_client_id = None;
    }
}
