use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use tokio::process::{Child, Command};

use crate::config::NgrokConfig;

pub struct NgrokTunnel {
    child: Child,
    public_url: String,
}

impl NgrokTunnel {
    pub async fn start(config: &NgrokConfig, bind_addr: &str) -> Result<Self> {
        let tunnel_name = format!(
            "tenex-telegram-gateway-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );

        let mut command = Command::new("ngrok");
        command
            .arg("http")
            .arg(bind_addr)
            .arg("--name")
            .arg(&tunnel_name)
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if let Some(requested_url) = config
            .requested_url
            .as_ref()
            .filter(|url| !url.trim().is_empty())
        {
            command.arg("--url").arg(requested_url);
        }

        let mut child = command
            .spawn()
            .context("Failed to start ngrok. Is the `ngrok` CLI installed and authenticated?")?;

        let public_url = poll_for_tunnel(config, &tunnel_name, bind_addr, &mut child).await?;
        Ok(Self { child, public_url })
    }

    pub fn public_url(&self) -> &str {
        &self.public_url
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        if self.child.id().is_some() {
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
        Ok(())
    }
}

async fn poll_for_tunnel(
    config: &NgrokConfig,
    tunnel_name: &str,
    bind_addr: &str,
    child: &mut Child,
) -> Result<String> {
    let client = reqwest::Client::new();
    let tunnel_api_url = format!("http://{}/api/tunnels", config.api_addr);
    let bind_port = extract_port(bind_addr);

    for _ in 0..60 {
        if let Some(status) = child
            .try_wait()
            .context("Failed to check ngrok process state")?
        {
            return Err(anyhow!(
                "ngrok exited before the tunnel became available (status: {})",
                status
            ));
        }

        if let Ok(response) = client.get(&tunnel_api_url).send().await {
            if response.status().is_success() {
                let body = response.text().await.unwrap_or_default();
                if let Ok(payload) = serde_json::from_str::<NgrokTunnelList>(&body) {
                    if let Some(tunnel) = payload.tunnels.into_iter().find(|tunnel| {
                        tunnel.name == tunnel_name
                            && tunnel.proto == "https"
                            && bind_port
                                .as_ref()
                                .map(|port| tunnel.config.addr.contains(&format!(":{port}")))
                                .unwrap_or(true)
                    }) {
                        return Ok(tunnel.public_url);
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    Err(anyhow!(
        "Timed out waiting for ngrok tunnel via {}",
        tunnel_api_url
    ))
}

fn extract_port(bind_addr: &str) -> Option<String> {
    bind_addr.rsplit(':').next().map(|value| value.to_string())
}

#[derive(Debug, Deserialize)]
struct NgrokTunnelList {
    tunnels: Vec<NgrokTunnelInfo>,
}

#[derive(Debug, Deserialize)]
struct NgrokTunnelInfo {
    name: String,
    public_url: String,
    proto: String,
    config: NgrokTunnelConfig,
}

#[derive(Debug, Deserialize)]
struct NgrokTunnelConfig {
    addr: String,
}
