//! Port of backend/src/index.ts's shared `.index.json`-per-download-folder
//! inventory helpers (`getInventoryPath`/`getInventory`/`updateInventory`/
//! `generateFileTree`). Used by filesystem.rs and (eventually) the
//! qBittorrent-status background-monitor logic, which hasn't been ported yet.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct FileTreeEntry {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DownloadEntry {
    #[serde(rename = "gameId", default)]
    pub game_id: Option<i64>,
    #[serde(rename = "folderName", default)]
    pub folder_name: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "fileTree", default)]
    pub file_tree: Vec<FileTreeEntry>,
    #[serde(default)]
    pub progress: f64,
    #[serde(rename = "addedAt", default)]
    pub added_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Inventory {
    #[serde(default)]
    pub downloads: HashMap<String, DownloadEntry>,
}

/// Port of `getInventoryPath()`. Returns None for an empty dir, matching the
/// Node version's `''` sentinel.
fn inventory_path(dir: &str) -> Option<PathBuf> {
    if dir.is_empty() {
        return None;
    }
    Some(Path::new(dir).join(".index.json"))
}

/// Port of `getInventory()`: missing file, unreadable file, or malformed
/// JSON all fall back to an empty inventory, same as the Node version.
pub fn get_inventory(dir: &str) -> Inventory {
    let Some(path) = inventory_path(dir) else {
        return Inventory::default();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return Inventory::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Port of `updateInventory()`: no-op if `dir` doesn't exist (matches the
/// Node version exactly, including that it silently does nothing rather
/// than erroring).
pub fn update_inventory(dir: &str, data: &Inventory) {
    if dir.is_empty() || !Path::new(dir).exists() {
        return;
    }
    let Some(path) = inventory_path(dir) else { return };
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = fs::write(path, json);
    }
}

/// Port of `generateFileTree()`: recursively lists every file under `dir`
/// with its size and path relative to the original root. Returns an empty
/// list on any read error, same as the Node version's try/catch.
pub fn generate_file_tree(dir: &Path, base_dir: &str) -> Vec<FileTreeEntry> {
    let mut results = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return results;
    };
    for entry in entries.flatten() {
        let res_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let rel_path = if base_dir.is_empty() {
            name.clone()
        } else {
            format!("{base_dir}/{name}")
        };
        if res_path.is_dir() {
            results.extend(generate_file_tree(&res_path, &rel_path));
        } else if let Ok(meta) = fs::metadata(&res_path) {
            results.push(FileTreeEntry { name, size: meta.len(), path: rel_path });
        }
    }
    results
}
