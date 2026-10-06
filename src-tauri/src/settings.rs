//! Port of backend/src/index.ts's settings load/save -- a single JSON file
//! at <USER_DATA_DIR>/config.json (same VRRookieDownloader -> VRSideForge ->
//! RooQuest migration dance as db.rs's db_path(), but a different base
//! directory).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(rename = "downloadPath")]
    pub download_path: String,
    #[serde(rename = "backupPath")]
    pub backup_path: String,
    /// Server source (go.srcdl1.xyz) access credential -- per explicit user
    /// decision, this is entered by the user themselves in Settings, never
    /// auto-detected or embedded in code. Empty string = not configured yet.
    #[serde(rename = "serverBaseUri")]
    pub server_base_uri: String,
    #[serde(rename = "serverPassword")]
    pub server_password: String,
    /// Twitch Developer app credentials for IGDB's OAuth client-credentials
    /// flow (IGDB sits behind Twitch's auth, not its own). Entered by the
    /// user themselves in Settings, same as the Server source's -- never
    /// guessed or embedded in code. Empty = IGDB metadata lookup is skipped
    /// entirely.
    #[serde(rename = "igdbClientId")]
    pub igdb_client_id: String,
    #[serde(rename = "igdbClientSecret")]
    pub igdb_client_secret: String,
    #[serde(rename = "translationLanguage")]
    pub translation_language: String,
    #[serde(rename = "interfaceLanguage")]
    pub interface_language: String,
    #[serde(rename = "offlineMode")]
    pub offline_mode: bool,
    /// When true, a background loop (spawned once at app startup, see
    /// `lib.rs`'s `spawn_auto_scrape_loops`) periodically runs that
    /// source's existing manual-scrape command on its own, on top of
    /// whatever the user triggers by hand from Settings. Off by default --
    /// this hits a real third-party site on a schedule, so it should be an
    /// explicit opt-in, not something that starts happening silently.
    #[serde(rename = "autoScrapeRutracker")]
    pub auto_scrape_rutracker: bool,
    #[serde(rename = "autoScrapeNnmclub")]
    pub auto_scrape_nnmclub: bool,
    pub start: bool,
    pub blacklist: Vec<String>,
}

/// <USER_DATA_DIR>: ~/Documents/RooQuest on Windows, ~/.local/share/RooQuest
/// elsewhere. Migrates in order from whichever older folder name actually
/// exists (VRRookieDownloader -> VRSideForge -> RooQuest, the app's three
/// names in order) so a settings file saved under any earlier name keeps
/// being found rather than silently resetting to defaults -- same logic as
/// db.rs's db_path(), just a different parent dir on non-Windows.
fn user_data_dir() -> PathBuf {
    let home = dirs::home_dir().expect("no home directory");

    #[cfg(target_os = "windows")]
    let (legacy_dir, old_dir, new_dir) = {
        let docs = home.join("Documents");
        (docs.join("VRRookieDownloader"), docs.join("VRSideForge"), docs.join("RooQuest"))
    };
    #[cfg(not(target_os = "windows"))]
    let (legacy_dir, old_dir, new_dir) = {
        let base = home.join(".local").join("share");
        (base.join("VRRookieDownloader"), base.join("VRSideForge"), base.join("RooQuest"))
    };

    if !new_dir.exists() {
        if old_dir.exists() {
            let _ = fs::rename(&old_dir, &new_dir);
        } else if legacy_dir.exists() {
            let _ = fs::rename(&legacy_dir, &new_dir);
        }
    }
    if !new_dir.exists() {
        let _ = fs::create_dir_all(&new_dir);
    }
    new_dir
}

fn config_path() -> PathBuf {
    user_data_dir().join("config.json")
}

fn default_games_dir() -> PathBuf {
    user_data_dir().join("games")
}

fn default_backup_dir() -> PathBuf {
    user_data_dir().join("backups")
}

/// The developer-shipped global blacklist (blacklist.json, bundled alongside
/// the app). We don't have an equivalent bundled file in desktop/ yet, so this
/// returns empty for now -- same shape/behavior as the Node version's fallback
/// when its blacklist.json is missing or invalid.
fn shipped_blacklist() -> Vec<String> {
    Vec::new()
}

fn defaults() -> Settings {
    let _ = fs::create_dir_all(default_games_dir());
    Settings {
        download_path: default_games_dir().to_string_lossy().to_string(),
        backup_path: default_backup_dir().to_string_lossy().to_string(),
        server_base_uri: String::new(),
        server_password: String::new(),
        igdb_client_id: String::new(),
        igdb_client_secret: String::new(),
        translation_language: "en".to_string(),
        interface_language: "en".to_string(),
        offline_mode: false,
        auto_scrape_rutracker: false,
        auto_scrape_nnmclub: false,
        start: true,
        blacklist: shipped_blacklist(),
    }
}

/// Port of `loadSettings()`. Merges saved config.json over the defaults,
/// and merges the saved blacklist with the shipped one (dedup + sort), same
/// as the original's self-healing merge -- except we don't rewrite the file
/// here on a blacklist diff (callers that need the on-disk file updated
/// should call `save_settings` explicitly; this keeps `load_settings` a pure
/// read, which is simpler to reason about from Rust command handlers).
pub fn load_settings() -> Settings {
    let def = defaults();
    let path = config_path();

    let Ok(raw) = fs::read_to_string(&path) else {
        return def;
    };
    let Ok(saved): Result<serde_json::Value, _> = serde_json::from_str(&raw) else {
        return def;
    };

    let mut merged = def.clone();
    if let Some(v) = saved.get("downloadPath").and_then(|v| v.as_str()) {
        merged.download_path = v.to_string();
    }
    if let Some(v) = saved.get("backupPath").and_then(|v| v.as_str()) {
        merged.backup_path = v.to_string();
    }
    if let Some(v) = saved.get("serverBaseUri").and_then(|v| v.as_str()) {
        merged.server_base_uri = v.to_string();
    }
    if let Some(v) = saved.get("serverPassword").and_then(|v| v.as_str()) {
        merged.server_password = v.to_string();
    }
    if let Some(v) = saved.get("igdbClientId").and_then(|v| v.as_str()) {
        merged.igdb_client_id = v.to_string();
    }
    if let Some(v) = saved.get("igdbClientSecret").and_then(|v| v.as_str()) {
        merged.igdb_client_secret = v.to_string();
    }
    if let Some(v) = saved.get("translationLanguage").and_then(|v| v.as_str()) {
        merged.translation_language = v.to_string();
    }
    if let Some(v) = saved.get("interfaceLanguage").and_then(|v| v.as_str()) {
        merged.interface_language = v.to_string();
    }
    if let Some(v) = saved.get("offlineMode").and_then(|v| v.as_bool()) {
        merged.offline_mode = v;
    }
    if let Some(v) = saved.get("autoScrapeRutracker").and_then(|v| v.as_bool()) {
        merged.auto_scrape_rutracker = v;
    }
    if let Some(v) = saved.get("autoScrapeNnmclub").and_then(|v| v.as_bool()) {
        merged.auto_scrape_nnmclub = v;
    }
    if let Some(v) = saved.get("start").and_then(|v| v.as_bool()) {
        merged.start = v;
    }

    let mut combined: Vec<String> = saved
        .get("blacklist")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    combined.extend(shipped_blacklist());
    combined.sort();
    combined.dedup();
    merged.blacklist = combined;

    merged
}

/// Port of `saveSettings()`: re-reads current settings, shallow-merges the
/// provided partial update, writes it back as pretty JSON.
pub fn save_settings(partial: &serde_json::Value) -> std::io::Result<Settings> {
    let mut current = load_settings();

    if let Some(v) = partial.get("downloadPath").and_then(|v| v.as_str()) {
        current.download_path = v.to_string();
        fs::create_dir_all(&current.download_path)?;
        let index_path = PathBuf::from(&current.download_path).join(".index.json");
        if !index_path.exists() {
            fs::write(&index_path, r#"{"downloads":{}}"#)?;
        }
    }
    if let Some(v) = partial.get("backupPath").and_then(|v| v.as_str()) {
        current.backup_path = v.to_string();
        fs::create_dir_all(&current.backup_path)?;
    }
    if let Some(v) = partial.get("serverBaseUri").and_then(|v| v.as_str()) {
        current.server_base_uri = v.to_string();
    }
    if let Some(v) = partial.get("serverPassword").and_then(|v| v.as_str()) {
        current.server_password = v.to_string();
    }
    if let Some(v) = partial.get("igdbClientId").and_then(|v| v.as_str()) {
        current.igdb_client_id = v.to_string();
    }
    if let Some(v) = partial.get("igdbClientSecret").and_then(|v| v.as_str()) {
        current.igdb_client_secret = v.to_string();
    }
    if let Some(v) = partial.get("translationLanguage").and_then(|v| v.as_str()) {
        current.translation_language = v.to_string();
    }
    if let Some(v) = partial.get("interfaceLanguage").and_then(|v| v.as_str()) {
        current.interface_language = v.to_string();
    }
    if let Some(v) = partial.get("offlineMode").and_then(|v| v.as_bool()) {
        current.offline_mode = v;
    }
    if let Some(v) = partial.get("autoScrapeRutracker").and_then(|v| v.as_bool()) {
        current.auto_scrape_rutracker = v;
    }
    if let Some(v) = partial.get("autoScrapeNnmclub").and_then(|v| v.as_bool()) {
        current.auto_scrape_nnmclub = v;
    }
    if let Some(v) = partial.get("start").and_then(|v| v.as_bool()) {
        current.start = v;
    }
    if let Some(v) = partial.get("blacklist").and_then(|v| v.as_array()) {
        current.blacklist = v.iter().filter_map(|x| x.as_str().map(String::from)).collect();
    }

    let json = serde_json::to_string_pretty(&current)?;
    fs::write(config_path(), json)?;
    Ok(current)
}
