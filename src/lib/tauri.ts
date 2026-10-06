// Typed wrappers around every real Tauri command this app has on the Rust
// side (see src-tauri/src/lib.rs's generate_handler! list). Keeping these
// in one place means a renamed/retyped Rust command only needs updating
// here, not in every component that calls it.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ---- Shared types (mirrors the Rust structs exactly) ----

export interface Game {
  id: number | null;
  title: string | null;
  translated_title: string | null;
  description: string | null;
  translated_description: string | null;
  magnet: string | null;
  post_url: string | null;
  tags: string | null;
  size: string | null;
  image_url: string | null;
  seeds: number;
  leeches: number;
  registered_at: string | null;
  torrent_downloads: number;
  wishlist: number;
  genre: string | null;
  developer: string | null;
  publisher: string | null;
  version: string | null;
  languages: string | null;
  play_modes: string | null;
  igdb_matched: number;
}

export interface Settings {
  downloadPath: string;
  backupPath: string;
  serverBaseUri: string;
  serverPassword: string;
  igdbClientId: string;
  igdbClientSecret: string;
  translationLanguage: string;
  interfaceLanguage: string;
  offlineMode: boolean;
  autoScrapeRutracker: boolean;
  autoScrapeNnmclub: boolean;
  start: boolean;
  blacklist: string[];
}

export interface StorageInfo {
  total: string;
  used: string;
  free: string;
  percentage: string;
}

export interface AdbAppsResult {
  apps: string[];
  storage: StorageInfo | null;
}

export interface BackupResult {
  pkg: string;
  apk_backed_up: boolean;
  obb_backed_up: boolean;
  backup_dir: string;
}

export interface FolderInfo {
  name: string;
  packageName: string | null;
  hasApk: boolean;
  isIndexed: boolean;
  status: string;
  gameId: number | null;
  /** Set (0-100, < 100) while a matching torrent is still downloading --
   *  a folder with this set is NOT actually ready to install yet, even
   *  though hasApk may already be true (the file exists but isn't
   *  finished being written). null/undefined means done, or not a
   *  tracked torrent at all. */
  downloadProgress: number | null;
}

export interface ScanResult {
  success: boolean;
  matchedCount: number;
  totalScanned: number;
}

export interface ScraperProgress {
  status: string;
  saved: number;
  skipped: number;
  failed: number;
}

export type SourceLabel = "main" | "nnmclub" | "server";

// ---- Library ----

export const getGames = () => invoke<Game[]>("get_games");
export const getGame = (id: number) => invoke<Game | null>("get_game", { id });
export const deleteGame = (id: number) => invoke<void>("delete_game", { id });
export const setWishlist = (id: number, wishlist: boolean) =>
  invoke<void>("set_wishlist", { id, wishlist });
export const updateGame = (
  id: number,
  fields: Partial<
    Pick<Game, "title" | "tags" | "genre" | "developer" | "publisher" | "version" | "languages" | "play_modes">
  >
) =>
  invoke<void>("update_game", {
    id,
    title: fields.title ?? null,
    tags: fields.tags ?? null,
    genre: fields.genre ?? null,
    developer: fields.developer ?? null,
    publisher: fields.publisher ?? null,
    version: fields.version ?? null,
    languages: fields.languages ?? null,
    playModes: fields.play_modes ?? null,
  });

// ---- Torrent sources (login + scraper tests) ----

export const openSourceLogin = (windowLabel: SourceLabel) =>
  invoke<void>("open_source_login", { windowLabel });

export const checkSourceLogin = (windowLabel: SourceLabel) =>
  invoke<boolean>("check_source_login", { windowLabel });

export const startScraperTest = () => invoke<string>("start_scraper_test");

// `keyword` is optional -- the backend's own default ("VR", searched across
// every PC-game genre forum) is the right choice for a real "scrape the
// whole site" run. Passing a specific title here is for the rare case of
// manually chasing down one game by name, not the normal manual-scrape
// button, so an explicit `null` is sent rather than silently coercing an
// empty string to "VR" behind the scenes.
export const startNnmclubScraperTest = (keyword?: string) =>
  invoke<string>("start_nnmclub_scraper_test", { keyword: keyword?.trim() || null });

export const onScraperProgress = (cb: (p: ScraperProgress) => void) =>
  listen<ScraperProgress>("scraper_progress", (e) => cb(e.payload));

// ---- IGDB metadata backfill (manual "scrape IGDB now") ----

export interface IgdbBackfillProgress {
  status: string;
  processed: number;
  total: number;
  updated: number;
  skipped: number;
  failed: number;
}

export const igdbBackfillAll = () => invoke<string>("igdb_backfill_all");

export const onIgdbBackfillProgress = (cb: (p: IgdbBackfillProgress) => void) =>
  listen<IgdbBackfillProgress>("igdb_backfill_progress", (e) => cb(e.payload));

// ---- Translation backfill (manual "retranslate library") ----

export interface TranslateBackfillProgress {
  status: string;
  processed: number;
  total: number;
  updated: number;
  skipped: number;
  failed: number;
}

export const translateBackfillAll = () => invoke<string>("translate_backfill_all");

export const onTranslateBackfillProgress = (cb: (p: TranslateBackfillProgress) => void) =>
  listen<TranslateBackfillProgress>("translate_backfill_progress", (e) => cb(e.payload));

// ---- Per-game "Translate now" (bump one game ahead of the backfill queue) ----

export const translateOne = (id: number) => invoke<Game>("translate_one", { id });

// ---- Settings ----

export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (partial: Partial<Settings>) =>
  invoke<Settings>("save_settings", { partial });

// ---- Sideload: ADB ----

export const adbDevices = () => invoke<string[]>("adb_devices");
export const adbCheckPath = () => invoke<boolean>("adb_check_path");
export const adbApps = (deviceId?: string) =>
  invoke<AdbAppsResult>("adb_apps", { deviceId: deviceId ?? null });
export const adbUninstall = (pkg: string, deviceId?: string) =>
  invoke<boolean>("adb_uninstall", { pkg, deviceId: deviceId ?? null });
export const adbInstall = (folderPath: string, deviceId?: string) =>
  invoke<void>("adb_install", { folderPath, deviceId: deviceId ?? null });
export const adbBackupApp = (pkg: string, deviceId?: string) =>
  invoke<BackupResult>("adb_backup_app", { pkg, deviceId: deviceId ?? null });

// ---- Sideload: local filesystem ----

export const fsFolders = () => invoke<FolderInfo[]>("fs_folders");
export const fsScanLocalDownloads = () => invoke<ScanResult>("fs_scan_local_downloads");
export const fsManualIndex = (folderName: string, gameId: number) =>
  invoke<void>("fs_manual_index", { folderName, gameId });
export const fsRemoveIndex = (folderName: string) =>
  invoke<boolean>("fs_remove_index", { folderName });

// ---- Sideload: qBittorrent + internal torrent engine ----
// (qbit_* talk directly to an external qBittorrent instance, used for
// settings/diagnostics display; torrent_* are this app's own engine-agnostic
// built-in engine and are what the actual download UI should call. Both
// exist on the Rust side; wiring real UI for either is follow-up work --
// declared here now, with real signatures, so it's ready when needed.)

export interface TorrentCheck {
  ok: boolean;
  [key: string]: unknown;
}

// Mirrors torrent::TorrentInfo exactly. `progress` is already 0-100 (both
// rqbit and qBittorrent backends normalize it before this crosses the
// bridge -- see torrent.rs's managed_to_info / qbittorrent.rs's mapping).
// `state` is a raw engine-specific string (rqbit: "live"/"paused"/etc,
// qBittorrent: "downloading"/"pausedDL"/etc) -- the UI only distinguishes
// paused vs. not, matching the design.
export interface TorrentInfo {
  hash: string;
  name: string;
  progress: number;
  state: string;
  engine: string;
  peers_connected: number;
  peers_seen: number;
  download_speed: string;
  upload_speed: string;
  error: string | null;
}

export const qbitCheck = () => invoke<TorrentCheck>("qbit_check");
export const qbitAddTorrent = (magnet: string, savepath: string) =>
  invoke<void>("qbit_add_torrent", { magnet, savepath });
export const qbitListTorrents = () => invoke<TorrentInfo[]>("qbit_list_torrents");
export const qbitTorrentAction = (hash: string, action: string) =>
  invoke<void>("qbit_torrent_action", { hash, action });

export const torrentAdd = (magnet: string, savepath: string) =>
  invoke<TorrentInfo>("torrent_add", { magnet, savepath });
export const torrentList = () => invoke<TorrentInfo[]>("torrent_list");
export const torrentAction = (hash: string, action: string) =>
  invoke<void>("torrent_action", { hash, action });

// ---- Session / cookies (RuTracker login state stored in the DB -- a
// separate, older mechanism from the per-window check_source_login live
// check added this session; both currently coexist) ----

export interface SessionCheck {
  ok: boolean;
  challenged: boolean;
  status: number;
}

export const sessionExists = () => invoke<boolean>("session_exists");
export const sessionCheck = () => invoke<SessionCheck>("session_check");
export const importCookies = (bbSession: string, cfClearance?: string, userAgent?: string) =>
  invoke<SessionCheck>("import_cookies", {
    bbSession,
    cfClearance: cfClearance ?? null,
    userAgent: userAgent ?? null,
  });
