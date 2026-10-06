//! The bridge the scrape-spike proved out: a source's window stays navigated
//! to that source's own site (real session, real cookies, real engine), and
//! instead of a separate HTTP client, we ask THAT page to fetch() a URL
//! itself and hand the result back to Rust over Tauri's IPC. This is the
//! two-way version of the spike's fire-and-forget overlay buttons.
//!
//! Originally this only ever talked to the single "main" (rutracker) window.
//! `sources.rs` now defines one window per indexed torrent source (rutracker,
//! nnmclub, server), each logged in and navigated independently -- so
//! `fetch_via_webview` takes a `window_label` to say which source's window
//! should do the fetching.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tokio::sync::oneshot;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FetchOutcome {
    pub status: u32,
    pub text: String,
    pub error: Option<String>,
}

pub struct PendingFetches {
    pub next_id: AtomicU64,
    pub waiting: Mutex<HashMap<u64, oneshot::Sender<FetchOutcome>>>,
}

impl Default for PendingFetches {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            waiting: Mutex::new(HashMap::new()),
        }
    }
}

/// Mutual-exclusion flags for the two source scrapers (rutracker, nnmclub).
/// The original Node backend's `startScraper()` began with `if (isRunning)
/// return;` -- a guard against the manual "Scrape now" button and the
/// auto-scrape background timer launching two copies of the same scraper at
/// once. This Rust port never carried that guard over, and a real run
/// confirmed the consequence directly from the terminal log: the SAME
/// "Oculus Quest games" query started twice, pages into each other,
/// mid-run, with no lock stopping it. Two scrapers racing the same site
/// under the same session at the same time is itself a plausible
/// contributor to any rate limiting rutracker's tracker.php endpoint
/// imposes, on top of just wasting a parallel set of requests.
pub struct ScraperGuards {
    pub rutracker_running: std::sync::atomic::AtomicBool,
    pub nnmclub_running: std::sync::atomic::AtomicBool,
}

impl Default for ScraperGuards {
    fn default() -> Self {
        Self {
            rutracker_running: std::sync::atomic::AtomicBool::new(false),
            nnmclub_running: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// RAII handle for one of `ScraperGuards`' flags: `try_acquire` flips the
/// flag to `true` and hands back a guard that flips it back to `false`
/// whenever it's dropped -- on a normal return, an early `return Err(...)`,
/// or a `?`-propagated error partway through a scrape -- so a crashed or
/// early-exiting run can never leave the flag stuck "running" forever.
pub struct RunGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl<'a> RunGuard<'a> {
    /// `None` means a run is already in progress; the caller should bail
    /// out (return an "already running" result) instead of starting a
    /// second one.
    pub fn try_acquire(flag: &'a std::sync::atomic::AtomicBool) -> Option<Self> {
        use std::sync::atomic::Ordering;
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Same idea as `FetchOutcome`, but for a binary response (a .torrent file,
/// specifically) -- `res.text()` would mangle non-UTF-8 bytes, so the
/// binary variant has the page's JS base64-encode the response instead.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BinaryFetchOutcome {
    pub status: u32,
    pub data_b64: String,
    pub error: Option<String>,
}

/// Separate pending-request map from `PendingFetches` (same shape, own ID
/// space) so a text fetch's `webview_fetch_result` can never be confused
/// with a binary fetch's `webview_fetch_binary_result`, even though both
/// can be in flight on the same window at once.
pub struct PendingBinaryFetches {
    pub next_id: AtomicU64,
    pub waiting: Mutex<HashMap<u64, oneshot::Sender<BinaryFetchOutcome>>>,
}

impl Default for PendingBinaryFetches {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            waiting: Mutex::new(HashMap::new()),
        }
    }
}

/// Called by the page's injected JS once its fetch() finishes. Resolves the
/// matching pending request in `fetch_via_webview` below.
#[tauri::command]
pub fn webview_fetch_result(
    state: tauri::State<PendingFetches>,
    id: u64,
    status: u32,
    text: String,
    error: Option<String>,
) {
    let sender = state.waiting.lock().unwrap().remove(&id);
    if let Some(sender) = sender {
        let _ = sender.send(FetchOutcome { status, text, error });
    }
}

/// Binary counterpart to `webview_fetch_result`, called by the injected JS
/// in `fetch_binary_via_webview` once its `arrayBuffer()` + base64 encode
/// finishes.
#[tauri::command]
pub fn webview_fetch_binary_result(
    state: tauri::State<PendingBinaryFetches>,
    id: u64,
    status: u32,
    data_b64: String,
    error: Option<String>,
) {
    let sender = state.waiting.lock().unwrap().remove(&id);
    if let Some(sender) = sender {
        let _ = sender.send(BinaryFetchOutcome { status, data_b64, error });
    }
}

/// Drives one of the per-source windows (which must already be navigated to
/// that source's own origin -- it's a same-origin fetch) to fetch `url` and
/// returns the resulting status + body. Mirrors what httpFetcher.ts/worker.ts
/// did with axios, except the request is made by the WebView's own engine,
/// not a separate HTTP client.
///
/// `window_label` selects which source's window does the fetching --
/// "main" (rutracker), "nnmclub", or "server" (see sources.rs). Defaults to
/// "main" when omitted, so existing callers from before multi-source support
/// keep working unchanged.
#[tauri::command]
pub async fn fetch_via_webview(
    app: AppHandle,
    state: tauri::State<'_, PendingFetches>,
    url: String,
    window_label: Option<String>,
    method: Option<String>,
    body: Option<String>,
) -> Result<FetchOutcome, String> {
    let label = window_label.as_deref().unwrap_or("main");
    let window: WebviewWindow = app
        .get_webview_window(label)
        .ok_or_else(|| format!("{label} window not found"))?;

    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    state.waiting.lock().unwrap().insert(id, tx);

    // JSON-encode everything that gets spliced into the script literal below.
    let url_json = serde_json::to_string(&url).map_err(|e| e.to_string())?;
    let method = method.unwrap_or_else(|| "GET".to_string());
    let method_json = serde_json::to_string(&method).map_err(|e| e.to_string())?;
    // Only POST (and friends) send a body; the fetch options object omits
    // `body` entirely for GET since some engines reject a GET with a body.
    // NNM-Club's real search is a POST (application/x-www-form-urlencoded)
    // -- its tracker.php reads $_POST only, so a GET query string with the
    // same `nm=` param is silently ignored and looks like "zero results".
    let fetch_opts_extra = match &body {
        Some(b) => {
            let body_json = serde_json::to_string(b).map_err(|e| e.to_string())?;
            format!(
                ", body: {body_json}, headers: {{ 'Content-Type': 'application/x-www-form-urlencoded' }}"
            )
        }
        None => String::new(),
    };

    let script = format!(
        r#"
        (function() {{
            fetch({url_json}, {{ credentials: 'include', method: {method_json}{fetch_opts_extra} }})
                .then(function(res) {{
                    return res.text().then(function(text) {{
                        window.__TAURI__.core.invoke('webview_fetch_result', {{
                            id: {id},
                            status: res.status,
                            text: text,
                            error: null
                        }});
                    }});
                }})
                .catch(function(e) {{
                    window.__TAURI__.core.invoke('webview_fetch_result', {{
                        id: {id},
                        status: 0,
                        text: '',
                        error: String(e && e.message ? e.message : e)
                    }});
                }});
        }})();
        "#,
        url_json = url_json,
        method_json = method_json,
        fetch_opts_extra = fetch_opts_extra,
        id = id,
    );

    window.eval(&script).map_err(|e| e.to_string())?;

    // Give up after 30s so a hung request can't wedge the scraper forever.
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
        .await
        .map_err(|_| "webview fetch timed out".to_string())?
        .map_err(|_| "webview fetch channel closed".to_string())?;

    if let Some(err) = &outcome.error {
        return Err(err.clone());
    }
    Ok(outcome)
}

/// Binary counterpart to `fetch_via_webview` -- used for the one case so
/// far where the response isn't text: NNM-Club's `download.php` link,
/// which is the raw `.torrent` file (bencode, not UTF-8). The page's JS
/// reads it as an `arrayBuffer()` and base64-encodes it before handing it
/// back over IPC (a plain `text()` read would corrupt any byte sequence
/// that isn't valid UTF-8, which a bencode file routinely contains).
/// `torrentfile::to_magnet` turns the decoded bytes into a magnet URI.
#[tauri::command]
pub async fn fetch_binary_via_webview(
    app: AppHandle,
    state: tauri::State<'_, PendingBinaryFetches>,
    url: String,
    window_label: Option<String>,
) -> Result<BinaryFetchOutcome, String> {
    let label = window_label.as_deref().unwrap_or("main");
    let window: WebviewWindow = app
        .get_webview_window(label)
        .ok_or_else(|| format!("{label} window not found"))?;

    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    state.waiting.lock().unwrap().insert(id, tx);

    let url_json = serde_json::to_string(&url).map_err(|e| e.to_string())?;

    let script = format!(
        r#"
        (function() {{
            fetch({url_json}, {{ credentials: 'include' }})
                .then(function(res) {{
                    return res.arrayBuffer().then(function(buf) {{
                        var bytes = new Uint8Array(buf);
                        var binary = '';
                        var chunkSize = 0x8000;
                        for (var i = 0; i < bytes.length; i += chunkSize) {{
                            binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunkSize));
                        }}
                        var b64 = btoa(binary);
                        window.__TAURI__.core.invoke('webview_fetch_binary_result', {{
                            id: {id},
                            status: res.status,
                            data_b64: b64,
                            error: null
                        }});
                    }});
                }})
                .catch(function(e) {{
                    window.__TAURI__.core.invoke('webview_fetch_binary_result', {{
                        id: {id},
                        status: 0,
                        data_b64: '',
                        error: String(e && e.message ? e.message : e)
                    }});
                }});
        }})();
        "#,
        url_json = url_json,
        id = id,
    );

    window.eval(&script).map_err(|e| e.to_string())?;

    // A .torrent download sometimes has to be generated server-side (NNM-Club
    // and similar trackers can be slow to respond), so this gets more
    // headroom than the plain text fetch's 30s.
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(60), rx)
        .await
        .map_err(|_| "webview binary fetch timed out".to_string())?
        .map_err(|_| "webview binary fetch channel closed".to_string())?;

    if let Some(err) = &outcome.error {
        return Err(err.clone());
    }
    Ok(outcome)
}

/// Lets the Rust side know the user is logged in / navigated where we need them,
/// and lets the frontend show progress. Not used by webview_fetch_result itself,
/// but kept here since it travels the same path -- emitted after a successful
/// scrape pass so the UI's listen() calls have something to react to.
pub fn emit_scraper_event(app: &AppHandle, event: &str, payload: impl Serialize + Clone) {
    let _ = app.emit(event, payload);
}
