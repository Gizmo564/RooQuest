//! Registry of indexed torrent sources. Each source gets its own hidden
//! WebviewWindow, independently navigated and logged in, so all three can
//! stay authenticated at the same time (the "one window per source"
//! architecture, chosen over a single window that switches origins, so that
//! scraping one source never knocks another's session off-screen).
//!
//! Every window is created hidden (`.visible(false)`) -- the user brings one
//! to the front with the "Log in" button next to that source in Settings
//! (wired to the `open_source_login` command below), rather than three
//! browser windows popping up unprompted at launch. Once logged in, a
//! window's session persists for as long as the app stays open; the button
//! just needs clicking again if a session expires.
//!
//! nnmclub.to's listing is now scraped for real (scrape.rs's
//! `parse_nnmclub_list_page`, ported from a confirmed-working reference
//! scraper). The "Server" source is a known dead end for now -- see its
//! own doc comment below.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorrentSourceDef {
    /// Also the Tauri WebviewWindow label this source's window is created
    /// with, and the `window_label` passed to `fetch_via_webview` to scrape it.
    pub window_label: &'static str,
    /// Human-readable name for settings/status UI.
    pub display_name: &'static str,
    /// Where the window is pointed at startup -- a login page when the site
    /// has one, so the user can log in the same way they already do for
    /// rutracker (type credentials into the window, same as any browser).
    pub start_url: &'static str,
    /// A same-origin path `check_source_login` fetches (via this source's
    /// own window, so it carries real cookies) to tell whether the user is
    /// actually logged in, via `scrape::page_needs_auth`. Empty string =
    /// no check implemented for this source (the Server source).
    pub check_path: &'static str,
}

pub const RUTRACKER: TorrentSourceDef = TorrentSourceDef {
    window_label: "main",
    display_name: "RuTracker",
    start_url: "https://rutracker.org/forum/login.php",
    check_path: "/forum/index.php",
};

pub const NNMCLUB: TorrentSourceDef = TorrentSourceDef {
    window_label: "nnmclub",
    display_name: "NNM-Club",
    start_url: "https://nnmclub.to/forum/login.php",
    check_path: "/forum/index.php",
};

/// The "Server" source (go.srcdl1.xyz) is currently BLOCKED, not just
/// unimplemented. Per the reference app it models (github.com/
/// deliciousmeatpop/vrcd), this isn't a torrent tracker at all: {baseUri,
/// password} is handed to `rclone` as an HTTP remote (`--http-url`) to pull
/// a password-protected `meta.7z` game-list archive (and, presumably,
/// per-game 7z archives the same way); `password` is a base64-encoded 7z
/// extraction password, not a login.
///
/// The actual blocker: go.srcdl1.xyz sits behind a Cloudflare rule that
/// requires a private `X-API-Key` header, baked into the vrcd app at build
/// time by its own developer (obfuscated, injected via their own CI secret
/// -- not in the public repo). Confirmed with real `rclone copyto
/// --http-url` requests both from this session's sandbox AND from the
/// user's own home network via their own Mac Terminal -- both got a flat
/// 403, with or without that header, matching what curl saw for the page
/// itself. Without that key nothing gets through: not curl, not rclone,
/// not a browser -- it isn't a per-user login, IP block, or
/// browser-fingerprint check. We don't have that key and aren't trying to
/// extract it from vrcd's obfuscation; this source stays blocked until the
/// user has their own legitimate way to get one. The window/settings
/// scaffolding below is kept (harmless, inert) in case that changes.
pub const SERVER: TorrentSourceDef = TorrentSourceDef {
    window_label: "server",
    display_name: "Server",
    start_url: "https://go.srcdl1.xyz/",
    check_path: "",
};

/// Every source that gets a login/access WebviewWindow -- currently all
/// three, since even the Server source's JSON-feed access has to go
/// through a real browser engine to clear Cloudflare.
pub const WINDOWED: &[TorrentSourceDef] = &[RUTRACKER, NNMCLUB, SERVER];
