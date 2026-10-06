//! Hybrid torrent engine: librqbit (native, in-process) as the primary
//! backend, with the existing qBittorrent WebUI client (qbittorrent.rs) as
//! an automatic, fully-optional fallback when rqbit fails to add/manage a
//! torrent. This replaces the qBittorrent-only flow the Node backend used.
//!
//! Design, per the chosen architecture (hybrid: rqbit primary, qBittorrent
//! fallback; automatic on rqbit failure; qBittorrent fully optional):
//!   - `TorrentEngineState` lazily starts an rqbit `librqbit::Session` the
//!     first time it's needed (not at app startup), rooted at whatever
//!     save path the caller passes in (the user's configured download path).
//!   - `add_torrent` tries rqbit first. If starting the rqbit session or
//!     adding the magnet fails, it automatically falls back to the existing
//!     qbittorrent.rs client -- qBittorrent is never required to be running
//!     for the app to work at all, only for that one fallback path.
//!   - Every torrent is identified to the frontend by its info-hash hex
//!     string, matching qBittorrent's own hash-based identification -- so
//!     `TorrentInfo.hash` means the same thing regardless of which engine
//!     produced it, and the frontend doesn't need to know which backend
//!     handled a given torrent.
//!   - `EngineRegistry` records which backend owns a given hash so
//!     pause/resume/delete route to the right place.
//!
//! API surface below was grep-verified against the real librqbit 9.0.1 (and
//! librqbit-core 9.0.1) source in ~/.cargo/registry, not taken from docs.rs
//! summaries alone:
//!   - `Session::{new, add_torrent, get, with_torrents, pause, unpause,
//!     delete}` -- src/session.rs (librqbit)
//!   - `librqbit::api::TorrentIdOrHash { Id(TorrentId), Hash(Id20) }`,
//!     `TorrentId = usize`, `impl From<TorrentId>`/`impl From<Id20>` for it
//!     -- src/api.rs:47-50,141,147 and src/session.rs:77 (librqbit)
//!   - `librqbit::torrent_state::ManagedTorrentHandle = Arc<ManagedTorrent>`
//!     -- src/torrent_state/mod.rs:649 (librqbit)
//!   - `Id20 = Id<20>`; `Id<N>::as_string(&self) -> String` (hex::encode);
//!     `impl FromStr for Id<N>` parses that same hex (or base32) back --
//!     src/hash_id.rs (librqbit-core)
//!   - `librqbit` does NOT re-export `Id20` at its own crate root (it
//!     re-exports `magnet`, `peer_id`, `torrent_metainfo` wholesale, but not
//!     `hash_id`), so `librqbit-core` is a direct dependency here too.
//!
//! Still NOT grep-verified (taken from docs.rs field/type listings only --
//! flagged here rather than stated as certain): `AddTorrentOptions`'s exact
//! field set (`output_folder`/`sub_folder: Option<String>`) and whether it
//! derives `Default`; `TorrentStats`'s exact field names
//! (`total_bytes`/`progress_bytes`/`state`). If `cargo check` reports a
//! mismatch on either, that's the first place to look.

use librqbit::{api::TorrentIdOrHash, AddTorrent, AddTorrentOptions, Session};
use librqbit_core::hash_id::Id20;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

use crate::qbittorrent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    Rqbit,
    Qbittorrent,
}

/// Tracks which engine owns each hash we've handed back to the frontend, so
/// a later pause/resume/delete call knows where to route. In-memory only for
/// now -- lost on app restart, at which point `torrent_action` falls back to
/// trying rqbit first (see below). Persisting this is a follow-up.
#[derive(Default)]
struct EngineRegistry {
    owners: HashMap<String, Engine>,
}

pub struct TorrentEngineState {
    rqbit: AsyncMutex<Option<Arc<Session>>>,
    registry: AsyncMutex<EngineRegistry>,
}

impl Default for TorrentEngineState {
    fn default() -> Self {
        Self {
            rqbit: AsyncMutex::new(None),
            registry: AsyncMutex::new(EngineRegistry::default()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TorrentInfo {
    pub hash: String,
    pub name: String,
    pub progress: f64,
    pub state: String,
    /// Which backend is actually running this torrent -- surfaced to the
    /// frontend for diagnostics/settings display. Not required for
    /// pause/resume/delete (those look the owner up internally).
    pub engine: String,
    /// Peers currently connected (librqbit's `peer_stats.live`) -- 0 doesn't
    /// necessarily mean broken, but if it stays 0 for a while alongside 0
    /// progress, that's the real "why is this stuck" signal (no reachable
    /// peers, not a download that's merely slow).
    pub peers_connected: u32,
    /// Peers librqbit has discovered via DHT/trackers/PEX at all, whether or
    /// not a connection succeeded -- 0 here points at DHT/tracker discovery
    /// itself failing (network/firewall), rather than peers being found but
    /// refusing connections.
    pub peers_seen: u32,
    /// Human-readable current transfer rates (e.g. "1.23 MiB/s"); qBittorrent
    /// torrents don't populate these today (fully-optional fallback engine),
    /// so they show "0.00 MiB/s" there.
    pub download_speed: String,
    pub upload_speed: String,
    /// librqbit's own last-known error for this torrent, if any (metadata
    /// fetch failure, disk I/O error, etc.) -- surfaced directly instead of
    /// a torrent silently sitting at 0% with no indication why.
    pub error: Option<String>,
}

/// Lazily starts (or returns the already-running) rqbit session, rooted at
/// the given default output folder.
async fn ensure_rqbit_session(
    state: &TorrentEngineState,
    default_output_folder: &PathBuf,
) -> anyhow::Result<Arc<Session>> {
    let mut guard = state.rqbit.lock().await;
    if let Some(s) = guard.as_ref() {
        return Ok(s.clone());
    }
    let session = Session::new(default_output_folder.clone()).await?;
    *guard = Some(session.clone());
    Ok(session)
}

/// Adds a magnet link, trying rqbit first and automatically falling back to
/// qBittorrent (if it's reachable) on any rqbit failure -- starting the
/// session, adding the torrent, or anything in between.
pub async fn add_torrent(
    state: &TorrentEngineState,
    http: &reqwest::Client,
    qbit: &qbittorrent::QbitState,
    magnet: &str,
    savepath: &str,
) -> Result<TorrentInfo, String> {
    match try_add_via_rqbit(state, magnet, savepath).await {
        Ok(info) => {
            state.registry.lock().await.owners.insert(info.hash.clone(), Engine::Rqbit);
            Ok(info)
        }
        Err(rqbit_err) => match qbittorrent::add_torrent(http, qbit, magnet, savepath).await {
            Ok(()) => {
                // qbittorrent.rs's add_torrent doesn't return the hash it was
                // given, so pull it back out of the magnet link itself --
                // the same hash qBittorrent will report in torrents/info.
                let hash = magnet_hash(magnet).unwrap_or_default();
                state.registry.lock().await.owners.insert(hash.clone(), Engine::Qbittorrent);
                Ok(TorrentInfo {
                    hash,
                    name: String::new(),
                    progress: 0.0,
                    state: "queued".to_string(),
                    engine: "qbittorrent".to_string(),
                    peers_connected: 0,
                    peers_seen: 0,
                    download_speed: "0.00 MiB/s".to_string(),
                    upload_speed: "0.00 MiB/s".to_string(),
                    error: None,
                })
            }
            Err(qbit_err) => Err(format!(
                "rqbit failed ({rqbit_err}); qBittorrent fallback also failed ({qbit_err})"
            )),
        },
    }
}

async fn try_add_via_rqbit(state: &TorrentEngineState, magnet: &str, savepath: &str) -> anyhow::Result<TorrentInfo> {
    let output_folder = PathBuf::from(savepath);
    let session = ensure_rqbit_session(state, &output_folder).await?;

    let opts = AddTorrentOptions {
        output_folder: Some(savepath.to_string()),
        ..Default::default()
    };
    let response = session.add_torrent(AddTorrent::from_url(magnet), Some(opts)).await?;

    let handle = response
        .into_handle()
        .ok_or_else(|| anyhow::anyhow!("rqbit returned a list-only response, not a managed torrent"))?;

    Ok(managed_to_info(&handle))
}

fn managed_to_info(handle: &librqbit::ManagedTorrent) -> TorrentInfo {
    let stats = handle.stats();
    let progress = if stats.total_bytes > 0 {
        (stats.progress_bytes as f64 / stats.total_bytes as f64 * 100.0 * 10.0).round() / 10.0
    } else {
        0.0
    };

    // `stats.live` is None while the torrent is still in the Initializing
    // state (metadata not resolved yet) or once it's Paused/Error -- there's
    // simply no live peer/speed snapshot to report in those states.
    let (peers_connected, peers_seen, download_speed, upload_speed) = match &stats.live {
        Some(live) => (
            live.snapshot.peer_stats.live,
            live.snapshot.peer_stats.seen,
            live.download_speed.to_string(),
            live.upload_speed.to_string(),
        ),
        None => (0, 0, "0.00 MiB/s".to_string(), "0.00 MiB/s".to_string()),
    };

    TorrentInfo {
        hash: handle.info_hash().as_string(),
        name: handle.name().unwrap_or_default(),
        progress,
        state: format!("{}", stats.state),
        engine: "rqbit".to_string(),
        peers_connected,
        peers_seen,
        download_speed,
        upload_speed,
        error: stats.error.clone(),
    }
}

/// Pulls the v1 `xt=urn:btih:<hash>` info-hash out of a magnet link, as a
/// lowercase hex string (same format `Id20::as_string()` produces). Returns
/// None if the magnet has no recognizable btih parameter.
fn magnet_hash(magnet: &str) -> Option<String> {
    for part in magnet.split('&') {
        let part = part.trim_start_matches('?');
        if let Some(idx) = part.find("xt=urn:btih:") {
            return Some(part[idx + "xt=urn:btih:".len()..].to_lowercase());
        }
    }
    None
}

pub async fn list_torrents(
    state: &TorrentEngineState,
    http: &reqwest::Client,
    qbit: &qbittorrent::QbitState,
) -> Vec<TorrentInfo> {
    // `with_torrents`'s callback is `Fn`, not `FnMut`, so it can't capture
    // `&mut out` directly -- route the push through a RefCell instead.
    let out_cell = std::cell::RefCell::new(Vec::new());

    if let Some(session) = state.rqbit.lock().await.as_ref() {
        session.with_torrents(|iter| {
            for (_idx, handle) in iter {
                out_cell.borrow_mut().push(managed_to_info(handle));
            }
        });
    }

    let mut out = out_cell.into_inner();

    // qBittorrent is fully optional: a failure here (not running) just
    // means we only report rqbit's torrents, which is correct.
    if let Ok(qbit_list) = qbittorrent::list_torrents(http, qbit).await {
        out.extend(qbit_list.into_iter().map(|t| TorrentInfo {
            hash: t.hash,
            name: t.name,
            progress: t.progress,
            state: t.state,
            engine: "qbittorrent".to_string(),
            // qBittorrent's Web API does have per-torrent peer/speed fields,
            // but qbittorrent.rs's own TorrentInfo doesn't capture them yet
            // (fully-optional fallback engine, not the focus so far).
            peers_connected: 0,
            peers_seen: 0,
            download_speed: "0.00 MiB/s".to_string(),
            upload_speed: "0.00 MiB/s".to_string(),
            error: None,
        }));
    }

    out
}

/// `action` is one of "pause" | "resume" | "delete" | "delete_drive" --
/// same vocabulary qbittorrent.rs's `torrent_action` already uses.
pub async fn torrent_action(
    state: &TorrentEngineState,
    http: &reqwest::Client,
    qbit: &qbittorrent::QbitState,
    hash: &str,
    action: &str,
) -> Result<(), String> {
    let owner = state.registry.lock().await.owners.get(hash).copied();

    match owner {
        Some(Engine::Qbittorrent) => qbittorrent::torrent_action(http, qbit, hash, action).await,
        // Default to rqbit when we don't have a recorded owner (e.g. after
        // an app restart, since the registry is in-memory only right now).
        Some(Engine::Rqbit) | None => rqbit_torrent_action(state, hash, action).await,
    }
}

async fn rqbit_torrent_action(state: &TorrentEngineState, hash: &str, action: &str) -> Result<(), String> {
    let guard = state.rqbit.lock().await;
    let session = guard.as_ref().ok_or_else(|| format!("no rqbit session running; unknown hash {hash}"))?;

    let id20 = Id20::from_str(hash).map_err(|e| format!("invalid torrent hash {hash}: {e}"))?;
    let id = TorrentIdOrHash::from(id20);

    match action {
        "pause" => {
            let handle = session
                .get(id)
                .ok_or_else(|| format!("rqbit has no torrent with hash {hash}"))?;
            session.pause(&handle).await.map_err(|e| e.to_string())
        }
        "resume" => {
            let handle = session
                .get(id)
                .ok_or_else(|| format!("rqbit has no torrent with hash {hash}"))?;
            session.unpause(&handle).await.map_err(|e| e.to_string())
        }
        "delete" => session.delete(id, false).await.map_err(|e| e.to_string()),
        "delete_drive" => session.delete(id, true).await.map_err(|e| e.to_string()),
        other => Err(format!("invalid action: {other}")),
    }
}
