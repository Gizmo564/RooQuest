//! Port of backend/src/index.ts's qBittorrent WebUI client (hardcoded
//! localhost:8080, admin/adminadmin -- not configurable in the original either).
//! The Node version used a global axios interceptor to auto-relogin on 401/403;
//! here we just retry once inline after a fresh login, which is the same
//! effective behavior without needing a global hook.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const QBIT_BASE: &str = "http://127.0.0.1:8080";
const QBIT_USER: &str = "admin";
const QBIT_PASS: &str = "adminadmin";

/// Holds the current qBittorrent WebUI session cookie (e.g. "SID=xxxx").
/// Mirrors the Node backend's module-level `let qbitCookie = ''`.
#[derive(Default)]
pub struct QbitState(pub Mutex<Option<String>>);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TorrentInfo {
    pub hash: String,
    pub name: String,
    pub progress: f64,
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TorrentCheck {
    #[serde(rename = "isRunning")]
    pub is_running: bool,
    #[serde(rename = "webUiWorking")]
    pub web_ui_working: bool,
}

async fn login(client: &reqwest::Client) -> Option<String> {
    let resp = client
        .post(format!("{QBIT_BASE}/api/v2/auth/login"))
        .form(&[("username", QBIT_USER), ("password", QBIT_PASS)])
        .send()
        .await
        .ok()?;

    let cookie_header = resp.headers().get("set-cookie")?.to_str().ok()?;
    // Same as the Node version: take just the "SID=xxxx" piece before the first ';'.
    let sid = cookie_header.split(';').next()?.to_string();
    Some(sid)
}

async fn ensure_cookie(client: &reqwest::Client, state: &QbitState) -> Option<String> {
    {
        let guard = state.0.lock().unwrap();
        if let Some(c) = guard.as_ref() {
            return Some(c.clone());
        }
    }
    let cookie = login(client).await?;
    *state.0.lock().unwrap() = Some(cookie.clone());
    Some(cookie)
}

/// Runs `f` with a valid cookie, retrying once after a fresh login if the
/// first attempt comes back 401/403 -- the inline equivalent of the Node
/// version's global axios auto-relogin interceptor.
async fn with_auth<F, Fut>(client: &reqwest::Client, state: &QbitState, f: F) -> Result<reqwest::Response, String>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    let cookie = ensure_cookie(client, state)
        .await
        .ok_or("could not log in to qBittorrent WebUI")?;

    let resp = f(cookie).await.map_err(|e| e.to_string())?;
    if resp.status() == 401 || resp.status() == 403 {
        let fresh = login(client).await.ok_or("re-login to qBittorrent WebUI failed")?;
        *state.0.lock().unwrap() = Some(fresh.clone());
        return f(fresh).await.map_err(|e| e.to_string());
    }
    Ok(resp)
}

/// Adds a magnet link to qBittorrent. Mirrors `sendToQbit()`: fire-and-forget
/// in the original (errors just logged); here we surface the error instead,
/// since the caller (a Tauri command) can report it rather than losing it to
/// a log file.
pub async fn add_torrent(client: &reqwest::Client, state: &QbitState, magnet: &str, savepath: &str) -> Result<(), String> {
    let magnet = magnet.to_string();
    let savepath = savepath.to_string();
    let resp = with_auth(client, state, move |cookie| {
        let magnet = magnet.clone();
        let savepath = savepath.clone();
        let client = client.clone();
        async move {
            client
                .post(format!("{QBIT_BASE}/api/v2/torrents/add"))
                .header("Cookie", cookie)
                .form(&[("urls", magnet.as_str()), ("savepath", savepath.as_str())])
                .send()
                .await
        }
    })
    .await?;

    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("qBittorrent returned status {}", resp.status()))
    }
}

/// Port of `GET /api/torrent/check`: a process check (best-effort, platform
/// dependent) plus a WebUI reachability check.
pub async fn check(client: &reqwest::Client, state: &QbitState) -> TorrentCheck {
    let is_running = process_is_running().await;

    let web_ui_working = ensure_cookie(client, state).await.is_some()
        && tokio::time::timeout(
            std::time::Duration::from_millis(2000),
            client.get(format!("{QBIT_BASE}/api/v2/app/version")).send(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    TorrentCheck { is_running, web_ui_working }
}

async fn process_is_running() -> bool {
    #[cfg(target_os = "windows")]
    {
        let out = tokio::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq qbittorrent.exe"])
            .output()
            .await;
        return out
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("qbittorrent.exe"))
            .unwrap_or(false);
    }
    #[cfg(not(target_os = "windows"))]
    {
        for name in ["qbittorrent", "qbittorrent-nox"] {
            if let Ok(out) = tokio::process::Command::new("pgrep").args(["-x", name]).output().await {
                if !String::from_utf8_lossy(&out.stdout).trim().is_empty() {
                    return true;
                }
            }
        }
        false
    }
}

/// Port of `GET /api/torrent/status`'s live half: fetches qBittorrent's own
/// `torrents/info`. Merging this with the local inventory (.index.json) is
/// NOT done here yet -- the inventory/download-folder bookkeeping from the
/// Node backend (WebTorrent metadata prefetch, .index.json, the 3s background
/// monitor) hasn't been ported yet. This returns qBittorrent's live view only.
pub async fn list_torrents(client: &reqwest::Client, state: &QbitState) -> Result<Vec<TorrentInfo>, String> {
    let resp = with_auth(client, state, move |cookie| {
        let client = client.clone();
        async move {
            client
                .get(format!("{QBIT_BASE}/api/v2/torrents/info"))
                .header("Cookie", cookie)
                .send()
                .await
        }
    })
    .await?;

    #[derive(Deserialize)]
    struct RawTorrent {
        hash: String,
        name: String,
        progress: f64,
        state: String,
    }

    let raw: Vec<RawTorrent> = resp.json().await.map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .map(|t| TorrentInfo {
            hash: t.hash,
            name: t.name,
            progress: (t.progress * 100.0 * 10.0).round() / 10.0,
            state: t.state,
        })
        .collect())
}

/// Port of `POST /api/torrent/action`. `action` is one of
/// "pause" | "resume" | "delete" | "delete_drive".
pub async fn torrent_action(client: &reqwest::Client, state: &QbitState, hash: &str, action: &str) -> Result<(), String> {
    let (endpoint, delete_files): (&str, Option<bool>) = match action {
        "pause" => ("torrents/pause", None),
        "resume" => ("torrents/resume", None),
        "delete" => ("torrents/delete", Some(false)),
        "delete_drive" => ("torrents/delete", Some(true)),
        other => return Err(format!("invalid action: {other}")),
    };

    let hash = hash.to_string();
    let endpoint = endpoint.to_string();
    let resp = with_auth(client, state, move |cookie| {
        let client = client.clone();
        let hash = hash.clone();
        let endpoint = endpoint.clone();
        async move {
            let mut form = vec![("hashes".to_string(), hash.clone())];
            if let Some(df) = delete_files {
                form.push(("deleteFiles".to_string(), df.to_string()));
            }
            client
                .post(format!("{QBIT_BASE}/api/v2/{endpoint}"))
                .header("Cookie", cookie)
                .form(&form)
                .send()
                .await
        }
    })
    .await?;

    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("qBittorrent returned status {}", resp.status()))
    }
}
