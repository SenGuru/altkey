//! Bridge to the local altkey agent: read status + toggle the tunnel over its
//! 127.0.0.1:8787 admin API, and manage the agent process lifecycle. In local
//! mode the agent's admin endpoints require no token.
use serde::Serialize;

const AGENT_BASE: &str = "http://127.0.0.1:8787";

#[derive(Serialize, Default)]
pub struct AgentStatus {
    pub running: bool,
    pub tunnel_up: bool,
    pub handle: Option<String>,
    pub reachable_url: Option<String>,
}

async fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap()
}

#[tauri::command]
pub async fn agent_status() -> AgentStatus {
    let client = http().await;
    // Is the agent up? Probe a cheap endpoint.
    let running = client
        .get(format!("{AGENT_BASE}/v1/models"))
        .send()
        .await
        .is_ok();
    if !running {
        return AgentStatus::default();
    }
    // Tunnel status (admin endpoint; open in local mode).
    let mut status = AgentStatus {
        running: true,
        ..Default::default()
    };
    if let Ok(resp) = client
        .get(format!("{AGENT_BASE}/admin/tunnel/status"))
        .send()
        .await
    {
        if let Ok(v) = resp.json::<serde_json::Value>().await {
            status.tunnel_up = v
                .get("tunnel_up")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            status.handle = v
                .get("handle")
                .and_then(|x| x.as_str())
                .map(String::from);
            status.reachable_url = status
                .handle
                .as_ref()
                .map(|h| format!("https://{h}.altkey.app/v1"));
        }
    }
    status
}

#[tauri::command]
pub async fn start_tunnel() -> Result<(), String> {
    http()
        .await
        .post(format!("{AGENT_BASE}/admin/tunnel/start"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn stop_tunnel() -> Result<(), String> {
    http()
        .await
        .post(format!("{AGENT_BASE}/admin/tunnel/stop"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Spawn the agent binary if it isn't already running. The binary path comes from
/// ALTKEY_AGENT_BIN (set at install time) or defaults to "altkey" on PATH.
#[tauri::command]
pub async fn start_agent() -> Result<(), String> {
    if agent_status().await.running {
        return Ok(());
    }
    let bin = std::env::var("ALTKEY_AGENT_BIN").unwrap_or_else(|_| "altkey".into());
    tokio::process::Command::new(bin)
        .spawn()
        .map_err(|e| format!("failed to start agent: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn open_web(path: String) -> Result<(), String> {
    let base =
        std::env::var("ALTKEY_WEB_URL").unwrap_or_else(|_| "https://altkey.app".into());
    let url = format!("{base}{path}");
    // Open in the default browser.
    open_url(&url).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
fn open_url(url: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn()
        .map(|_| ())
}

#[cfg(not(target_os = "windows"))]
fn open_url(url: &str) -> std::io::Result<()> {
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(cmd).arg(url).spawn().map(|_| ())
}
