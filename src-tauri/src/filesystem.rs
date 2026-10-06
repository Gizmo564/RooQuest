//! Port of backend/src/index.ts's 4 filesystem routes: `/api/filesystem/folders`,
//! `/scan-local-downloads`, `/manual-index`, `/remove-index`. All operate on
//! a caller-supplied download folder path and its `.index.json` inventory
//! (inventory.rs) -- callers (the Tauri commands in lib.rs) pass in
//! `settings::load_settings().download_path`, matching the Node version's
//! `globalDownloadPath`.

use crate::inventory::{self, DownloadEntry, FileTreeEntry};
use regex::Regex;
use std::fs;

#[derive(Debug, serde::Serialize, Clone)]
pub struct FolderInfo {
    pub name: String,
    #[serde(rename = "packageName")]
    pub package_name: Option<String>,
    #[serde(rename = "hasApk")]
    pub has_apk: bool,
    #[serde(rename = "isIndexed")]
    pub is_indexed: bool,
    pub status: String,
    #[serde(rename = "gameId")]
    pub game_id: Option<i64>,
    /// Set when a torrent engine (rqbit or qBittorrent) reports an
    /// in-progress torrent whose name matches this folder, and that
    /// torrent isn't done yet (< 100%). `folders()` used to decide
    /// "ready to install" purely from whether an .apk file was present on
    /// disk -- but both engines create/pre-allocate the destination file
    /// as soon as a torrent starts, well before it's actually fully
    /// written, so a still-downloading game showed an "Install" button
    /// immediately and installing it would push a truncated/corrupt APK.
    /// None means either fully downloaded, or not a tracked torrent at all
    /// (e.g. manually indexed / the engine isn't reachable) -- callers
    /// should only treat a folder as ready when this is None AND
    /// `has_apk` is true.
    #[serde(rename = "downloadProgress")]
    pub download_progress: Option<f64>,
}

/// A package-name-shaped string: starts with a letter, then alnum, then a
/// dot followed by a letter -- e.g. "com.example.app". Matches the Node
/// version's `/^[a-zA-Z][a-zA-Z0-9]*\.[a-zA-Z]/` heuristic exactly.
fn looks_like_package_name(s: &str) -> bool {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = PATTERN.get_or_init(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9]*\.[a-zA-Z]").unwrap());
    re.is_match(s)
}

fn strip_bracket_prefix(s: &str) -> String {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = PATTERN.get_or_init(|| Regex::new(r"^\[.*?\]\s*").unwrap());
    re.replace(s, "").to_string()
}

fn normalize(s: &str) -> String {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = PATTERN.get_or_init(|| Regex::new(r"[\s_\-\[\]().]+").unwrap());
    re.replace_all(&s.to_lowercase(), "").to_string()
}

/// Port of `GET /api/filesystem/folders`. Lists each top-level directory
/// (or loose .apk file) in `download_path`, guessing its package name via
/// the same filename heuristic the Node version used, and cross-referencing
/// the `.index.json` inventory for indexed/status/gameId.
pub fn folders(download_path: &str, active_torrents: &std::collections::HashMap<String, f64>) -> Vec<FolderInfo> {
    if download_path.is_empty() {
        return Vec::new();
    }
    let target = std::fs::canonicalize(download_path).unwrap_or_else(|_| std::path::PathBuf::from(download_path));
    if !target.exists() {
        return Vec::new();
    }

    let Ok(entries) = fs::read_dir(&target) else {
        return Vec::new();
    };

    let inv = inventory::get_inventory(&target.to_string_lossy());

    let mut result: Vec<FolderInfo> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            let is_dir = path.is_dir();
            let is_apk_file = path.is_file() && name.ends_with(".apk");
            if !is_dir && !is_apk_file {
                return None;
            }

            let mut has_apk = is_apk_file;
            let mut package_name = None;

            if is_apk_file {
                let without_ext = name.trim_end_matches(".apk");
                if looks_like_package_name(without_ext) {
                    package_name = Some(without_ext.to_string());
                }
            } else if let Ok(sub_entries) = fs::read_dir(&path) {
                if let Some(apk_file) = sub_entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .find(|f| f.ends_with(".apk"))
                {
                    has_apk = true;
                    let without_ext = apk_file.trim_end_matches(".apk");
                    if looks_like_package_name(without_ext) {
                        package_name = Some(without_ext.to_string());
                    }
                }
            }

            let inv_item = inv.downloads.values().find(|d| d.folder_name == name);
            let is_indexed = inv_item.is_some();
            let status = inv_item
                .map(|d| d.status.clone())
                .unwrap_or_else(|| if has_apk { "unindexed".to_string() } else { "unknown".to_string() });
            let game_id = inv_item.and_then(|d| d.game_id);

            // A torrent whose name matches this folder and isn't at 100% yet
            // means the folder's contents (apk included) are still being
            // written -- not ready to install regardless of has_apk.
            let download_progress = active_torrents.get(&name).copied().filter(|p| *p < 100.0);

            Some(FolderInfo { name, package_name, has_apk, is_indexed, status, game_id, download_progress })
        })
        .collect();

    result.sort_by(|a, b| strip_bracket_prefix(&a.name).to_lowercase().cmp(&strip_bracket_prefix(&b.name).to_lowercase()));

    result
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct ScanResult {
    pub success: bool,
    #[serde(rename = "matchedCount")]
    pub matched_count: u32,
    #[serde(rename = "totalScanned")]
    pub total_scanned: u32,
}

/// Port of `POST /api/filesystem/scan-local-downloads`: for every
/// un-indexed top-level item in `download_path`, tries to match it to a
/// game by a prefix-substring heuristic on title, and if found, writes a
/// synthetic `local_<gameId>_<hex>` entry into `.index.json`.
pub fn scan_local_downloads(conn: &rusqlite::Connection, download_path: &str) -> Result<ScanResult, String> {
    if download_path.is_empty() || !std::path::Path::new(download_path).exists() {
        return Err("Download folder is invalid or not configured.".to_string());
    }

    let mut inv = inventory::get_inventory(download_path);
    let entries = fs::read_dir(download_path).map_err(|e| e.to_string())?;
    let local_items: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            !name.starts_with('.')
                && (e.path().is_dir() || (e.path().is_file() && name.ends_with(".apk")))
        })
        .collect();

    let mut matched_count = 0u32;
    let mut inventory_changed = false;

    for item in &local_items {
        let item_name = item.file_name().to_string_lossy().to_string();
        let already_indexed = inv.downloads.values().any(|d| d.folder_name == item_name);
        if already_indexed {
            continue;
        }

        let clean_item_name = item_name.trim_end_matches(".apk").to_lowercase();
        let normalized_search = normalize(&clean_item_name);
        let search_prefix: String = normalized_search.chars().take(15).collect();
        if search_prefix.len() < 3 {
            continue;
        }

        let pattern_a = format!("%{clean_item_name}%");
        let pattern_b = format!("%{search_prefix}%");
        let game = crate::db::find_game_by_title_like(conn, &pattern_a, &pattern_b).map_err(|e| e.to_string())?;

        if let Some((game_id, _title)) = game {
            let hash_suffix: String = item_name.bytes().map(|b| format!("{b:02x}")).collect::<String>().chars().take(8).collect();
            let virtual_hash = format!("local_{game_id}_{hash_suffix}");

            let full_path = std::path::Path::new(download_path).join(&item_name);
            let file_tree = if full_path.is_dir() {
                inventory::generate_file_tree(&full_path, "")
            } else {
                let size = fs::metadata(&full_path).map(|m| m.len()).unwrap_or(0);
                vec![FileTreeEntry { name: item_name.clone(), size, path: item_name.clone() }]
            };

            inv.downloads.insert(
                virtual_hash.clone(),
                DownloadEntry {
                    game_id: Some(game_id),
                    folder_name: item_name.clone(),
                    hash: virtual_hash,
                    status: "concluido".to_string(),
                    file_tree,
                    progress: 100.0,
                    added_at: chrono::Utc::now().to_rfc3339(),
                },
            );
            inventory_changed = true;
            matched_count += 1;
        }
    }

    if inventory_changed {
        inventory::update_inventory(download_path, &inv);
    }

    Ok(ScanResult { success: true, matched_count, total_scanned: local_items.len() as u32 })
}

/// Port of `POST /api/filesystem/manual-index`: writes a synthetic
/// `manual_<gameId>_<hex>` entry for a folder/file the user picked by hand.
pub fn manual_index(download_path: &str, folder_name: &str, game_id: i64) -> Result<(), String> {
    if download_path.is_empty() || folder_name.is_empty() {
        return Err("Insufficient data".to_string());
    }

    let mut inv = inventory::get_inventory(download_path);
    let full_path = std::path::Path::new(download_path).join(folder_name);
    if !full_path.exists() {
        return Err("Path not found".to_string());
    }

    let file_tree = if full_path.is_dir() {
        inventory::generate_file_tree(&full_path, "")
    } else {
        let size = fs::metadata(&full_path).map(|m| m.len()).unwrap_or(0);
        vec![FileTreeEntry { name: folder_name.to_string(), size, path: folder_name.to_string() }]
    };

    let hash_suffix: String = folder_name.bytes().map(|b| format!("{b:02x}")).collect::<String>().chars().take(8).collect();
    let virtual_hash = format!("manual_{game_id}_{hash_suffix}");

    inv.downloads.insert(
        virtual_hash.clone(),
        DownloadEntry {
            game_id: Some(game_id),
            folder_name: folder_name.to_string(),
            hash: virtual_hash,
            status: "concluido".to_string(),
            file_tree,
            progress: 100.0,
            added_at: chrono::Utc::now().to_rfc3339(),
        },
    );

    inventory::update_inventory(download_path, &inv);
    Ok(())
}

/// Port of `POST /api/filesystem/remove-index`: removes every inventory
/// entry whose `folderName` matches (the Node version also just matches by
/// name, not by hash, so a folder re-added under a different hash scheme
/// still gets cleaned up).
pub fn remove_index(download_path: &str, folder_name: &str) -> Result<bool, String> {
    if download_path.is_empty() || folder_name.is_empty() {
        return Err("Insufficient data".to_string());
    }

    let mut inv = inventory::get_inventory(download_path);
    let to_remove: Vec<String> = inv
        .downloads
        .iter()
        .filter(|(_, d)| d.folder_name == folder_name)
        .map(|(hash, _)| hash.clone())
        .collect();

    let removed = !to_remove.is_empty();
    for hash in to_remove {
        inv.downloads.remove(&hash);
    }

    if removed {
        inventory::update_inventory(download_path, &inv);
    }
    Ok(removed)
}
