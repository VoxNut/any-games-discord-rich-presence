use crate::discord_ipc::packet::{HandshakePayload, Opcode};
use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::debug;

pub enum IpcStream {
    #[cfg(windows)]
    Windows(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

impl IpcStream {
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        match self {
            #[cfg(windows)]
            IpcStream::Windows(stream) => {
                stream.read_exact(buf).await.context("Failed reading from Windows named pipe")?;
            }
            #[cfg(unix)]
            IpcStream::Unix(stream) => {
                stream.read_exact(buf).await.context("Failed reading from Unix domain socket")?;
            }
        }
        Ok(())
    }

    pub async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        match self {
            #[cfg(windows)]
            IpcStream::Windows(stream) => {
                stream.write_all(buf).await.context("Failed writing to Windows named pipe")?;
                stream.flush().await.context("Failed flushing Windows named pipe")?;
            }
            #[cfg(unix)]
            IpcStream::Unix(stream) => {
                stream.write_all(buf).await.context("Failed writing to Unix domain socket")?;
                stream.flush().await.context("Failed flushing Unix domain socket")?;
            }
        }
        Ok(())
    }
}

pub struct IpcConnection {
    stream: IpcStream,
}

impl IpcConnection {
    /// Try connecting to Discord IPC on the local machine
    pub async fn connect(client_id: &str) -> Result<Self> {
        let stream = Self::find_and_open_stream().await?;
        let mut conn = Self { stream };

        // Perform Handshake
        conn.handshake(client_id).await?;
        Ok(conn)
    }

    async fn find_and_open_stream() -> Result<IpcStream> {
        #[cfg(windows)]
        {
            for i in 0..10 {
                let pipe_name = format!(r"\\.\pipe\discord-ipc-{}", i);
                match tokio::net::windows::named_pipe::ClientOptions::new().open(&pipe_name) {
                    Ok(client) => {
                        debug!("Connected to Discord IPC pipe: {}", pipe_name);
                        return Ok(IpcStream::Windows(client));
                    }
                    Err(_) => continue,
                }
            }
            bail!("Could not connect to any Discord IPC named pipe (\\.\\pipe\\discord-ipc-0..9). Is Discord running?");
        }

        #[cfg(unix)]
        {
            let mut candidate_dirs: Vec<std::path::PathBuf> = Vec::new();

            if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                candidate_dirs.push(std::path::PathBuf::from(runtime_dir));
            }
            if let Ok(tmpdir) = std::env::var("TMPDIR") {
                candidate_dirs.push(std::path::PathBuf::from(tmpdir));
            }
            if let Ok(tmp) = std::env::var("TMP") {
                candidate_dirs.push(std::path::PathBuf::from(tmp));
            }
            if let Ok(temp) = std::env::var("TEMP") {
                candidate_dirs.push(std::path::PathBuf::from(temp));
            }
            candidate_dirs.push(std::path::PathBuf::from("/tmp"));

            // Flatpak paths
            if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                candidate_dirs.push(PathBuf::from(runtime_dir).join("app/com.discordapp.Discord"));
                candidate_dirs.push(PathBuf::from(runtime_dir).join("app/com.discordapp.DiscordCanary"));
            }

            for dir in candidate_dirs {
                for i in 0..10 {
                    let socket_path = dir.join(format!("discord-ipc-{}", i));
                    if socket_path.exists() {
                        match tokio::net::UnixStream::connect(&socket_path).await {
                            Ok(stream) => {
                                debug!("Connected to Discord IPC socket: {:?}", socket_path);
                                return Ok(IpcStream::Unix(stream));
                            }
                            Err(_) => continue,
                        }
                    }
                }
            }
            bail!("Could not find an active Discord Unix socket. Is Discord running?");
        }
    }

    async fn handshake(&mut self, client_id: &str) -> Result<()> {
        let payload = HandshakePayload {
            v: 1,
            client_id: client_id.to_string(),
        };

        let json = serde_json::to_string(&payload)?;
        self.send_packet(Opcode::Handshake, &json).await?;

        // Read handshake response
        match self.recv_packet().await {
            Ok((opcode, resp_json)) => {
                debug!("Handshake response (opcode {:?}): {}", opcode, resp_json);
                if resp_json.contains("\"evt\":\"ERROR\"") || resp_json.contains("\"code\":") {
                    debug!("Discord handshake message: {}", resp_json);
                }
            }
            Err(e) => {
                debug!("Handshake response read: {:#}", e);
            }
        }

        Ok(())
    }

    pub async fn send_packet(&mut self, opcode: Opcode, payload: &str) -> Result<()> {
        let op_bytes = (opcode as u32).to_le_bytes();
        let len_bytes = (payload.len() as u32).to_le_bytes();

        let mut header = Vec::with_capacity(8 + payload.len());
        header.extend_from_slice(&op_bytes);
        header.extend_from_slice(&len_bytes);
        header.extend_from_slice(payload.as_bytes());

        self.stream.write_all(&header).await?;
        Ok(())
    }

    pub async fn recv_packet(&mut self) -> Result<(Opcode, String)> {
        let mut header = [0u8; 8];
        self.stream.read_exact(&mut header).await?;

        let opcode = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;

        if length == 0 {
            return Ok((Opcode::from(opcode), String::new()));
        }

        let mut payload_buf = vec![0u8; length];
        self.stream.read_exact(&mut payload_buf).await?;

        let payload_str = String::from_utf8(payload_buf).context("Received invalid UTF-8 from Discord IPC")?;
        Ok((Opcode::from(opcode), payload_str))
    }

    pub async fn close(&mut self) -> Result<()> {
        let _ = self.send_packet(Opcode::Close, "{}").await;
        Ok(())
    }
}
