mod adb;
mod db;
mod eta;
mod filesystem;
mod igdb;
mod images;
mod inventory;
mod qbittorrent;
mod scrape;
mod session;
mod settings;
mod sideload;
mod torrent;
mod torrentfile;
mod translate;
mod sources;
mod webview_fetch;

use serde::Serialize;
use std::sync::Mutex;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use webview_fetch::{
    fetch_binary_via_webview, fetch_via_webview, webview_fetch_binary_result, webview_fetch_result,
    PendingBinaryFetches, PendingFetches, RunGuard, ScraperGuards,
};

/// Mirrors the overlay the scrape-spike injected, minus the test buttons --
/// this is just a visible "you're on the right page" affordance while we build
/// out the real frontend. The real app's UI will eventually run in a *different*
/// (hidden) webview; this window stays dedicated to being "the real browser."
const KEEPALIVE_JS: &str = r#"
(function () {
  if (window.__vrsf_marked__) return;
  window.__vrsf_marked__ = true;
  console.log('[RooQuest] WebView session ready for', location.href);
})();
"#;

/// How often each source's background auto-scrape loop (see
/// `spawn_auto_scrape_loops`) wakes up to check whether it's turned on and,
/// if so, run one pass. Deliberately the same small batch
/// (`start_scraper_test`/`start_nnmclub_scraper_test` each take just the
/// first few new items per run) repeated periodically, rather than a big
/// catalog crawl -- gentler on the site, and it just keeps compounding on
/// its own as long as the app stays open.
const AUTO_SCRAPE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(4 * 60 * 60);
/// Short delay before the very first auto-scrape attempt after launch, so
/// a freshly-opened app doesn't sit for a full `AUTO_SCRAPE_INTERVAL`
/// before it does anything, but also isn't racing the source windows'
/// own navigation/login on the very first tick.
const AUTO_SCRAPE_INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Serialize, Clone)]
struct ScraperProgress {
    status: String,
    saved: u32,
    skipped: u32,
    failed: u32,
}

/// Best-effort IGDB metadata enrichment for a newly-inserted game. Looks up
/// the user's IGDB credentials in Settings; if either is blank (IGDB not
/// configured), this is a silent no-op. Any lookup failure (bad credentials,
/// network error, no match) is swallowed here -- this runs right after a
/// scrape saved a real row, and a flaky third-party metadata lookup should
/// never make that save look like it failed.
async fn try_fetch_igdb_metadata(
    http: &reqwest::Client,
    db_state: &db::DbState,
    game_id: i64,
    title: &str,
) {
    let settings = settings::load_settings();
    if settings.igdb_client_id.trim().is_empty() || settings.igdb_client_secret.trim().is_empty() {
        return;
    }

    match igdb::fetch_metadata(http, &settings.igdb_client_id, &settings.igdb_client_secret, title).await {
        Ok(Some(meta)) => {
            let Ok(conn) = db_state.0.lock() else { return };
            let _ = db::apply_igdb_metadata(
                &conn,
                game_id,
                meta.description.as_deref(),
                meta.genre.as_deref(),
                meta.developer.as_deref(),
                meta.publisher.as_deref(),
                meta.image_url.as_deref(),
            );
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("[igdb] metadata lookup failed for '{title}': {e}");
        }
    }
}

/// Returns everything currently in the `games` table.
#[tauri::command]
fn get_games(db: tauri::State<db::DbState>) -> Result<Vec<db::Game>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    db::get_all_games(&conn).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
struct IgdbBackfillProgress {
    status: String,
    processed: u32,
    total: u32,
    updated: u32,
    skipped: u32,
    failed: u32,
}

#[derive(Serialize, Clone)]
struct TranslateBackfillProgress {
    status: String,
    processed: u32,
    total: u32,
    updated: u32,
    skipped: u32,
    failed: u32,
}

/// Manual "retranslate library" action -- walks every game whose
/// translated_description still exactly equals its raw (Russian)
/// description, which is how a failed/never-attempted translation shows up
/// (translate::translate_text falls back to the original text rather than
/// leaving the field blank). Re-running this after Settings' translation
/// language changes, or after the free translate endpoint rate-limited a
/// big scrape run, fixes those rows without re-scraping. Paced the same way
/// as the per-scrape path (translate.rs already paces/retries each request).
#[tauri::command]
async fn translate_backfill_all(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, db::DbState>,
    http: tauri::State<'_, reqwest::Client>,
) -> Result<String, String> {
    let target_lang = settings::load_settings().translation_language;

    let games = {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        db::get_all_games(&conn).map_err(|e| e.to_string())?
    };
    let total = games.len() as u32;

    let mut processed = 0u32;
    let mut updated = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;
    // If the translate endpoint is still blocking this IP (see translate.rs),
    // every single call will fail the same way -- there's no point grinding
    // through the rest of a 1000+ game library at several seconds per game
    // just to find that out. Stop early after a run of consecutive failures
    // instead of silently burning hours.
    const MAX_CONSECUTIVE_FAILURES: u32 = 5;
    let mut consecutive_failures = 0u32;
    let mut stopped_early = false;

    for game in games {
        processed += 1;

        let (Some(id), Some(title), Some(description)) =
            (game.id, game.title.clone(), game.description.clone())
        else {
            skipped += 1;
            continue;
        };

        let already_translated = game
            .translated_description
            .as_deref()
            .is_some_and(|t| t != description);
        if already_translated {
            skipped += 1;
            continue;
        }

        let _ = app.emit(
            "translate_backfill_progress",
            TranslateBackfillProgress {
                status: format!("Translating {title}..."),
                processed,
                total,
                updated,
                skipped,
                failed,
            },
        );

        let new_title = translate::translate_text(&http, &title, &target_lang).await;
        let new_description = translate::translate_text(&http, &description, &target_lang).await;

        if new_description == description {
            // Still unchanged -- translation failed again (rate-limited,
            // offline, etc.) rather than the text genuinely being identical
            // in both languages. Don't write it back as "done".
            failed += 1;
            consecutive_failures += 1;
            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                stopped_early = true;
                break;
            }
            continue;
        }
        consecutive_failures = 0;

        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        let _ = db::apply_translation(&conn, id, &new_title, &new_description);
        drop(conn);
        updated += 1;
    }

    let final_status = if stopped_early {
        format!(
            "Stopped early after {MAX_CONSECUTIVE_FAILURES} translations in a row failed -- the translate endpoint still looks blocked. Try again later."
        )
    } else {
        "Finished".to_string()
    };
    let _ = app.emit(
        "translate_backfill_progress",
        TranslateBackfillProgress {
            status: final_status,
            processed,
            total,
            updated,
            skipped,
            failed,
        },
    );

    if stopped_early {
        Ok(format!(
            "stopped early (still rate-limited): updated={updated} skipped={skipped} failed={failed} processed={processed}/{total}"
        ))
    } else {
        Ok(format!("updated={updated} skipped={skipped} failed={failed} total={total}"))
    }
}

/// Per-game "Translate now" button -- bumps a single game to the front of
/// the line instead of waiting on the slow, heavily-paced full-library
/// backfill (translate_backfill_all). Does the same translate + write as
/// that backfill, just for one row, so a user who notices one specific
/// untranslated game doesn't have to wait for (or re-run) the whole queue.
#[tauri::command]
async fn translate_one(
    db_state: tauri::State<'_, db::DbState>,
    http: tauri::State<'_, reqwest::Client>,
    id: i64,
) -> Result<db::Game, String> {
    let target_lang = settings::load_settings().translation_language;

    let game = {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        db::get_game_by_id(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no game with id {id}"))?
    };

    let title = game.title.clone().unwrap_or_default();
    let description = game.description.clone().unwrap_or_default();

    let new_title = translate::translate_text(&http, &title, &target_lang).await;
    let new_description = translate::translate_text(&http, &description, &target_lang).await;

    if new_description == description {
        return Err(
            "Translation failed (the free translate service may still be rate-limited) -- try again in a bit".to_string(),
        );
    }

    let conn = db_state.0.lock().map_err(|e| e.to_string())?;
    db::apply_translation(&conn, id, &new_title, &new_description).map_err(|e| e.to_string())?;
    db::get_game_by_id(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("game {id} vanished after translating"))
}

/// Manual "scrape IGDB now" button -- walks every game currently in the
/// library and, for any that are still missing IGDB-sourced fields
/// (description/genre/developer/publisher/cover), looks them up and fills
/// in the gaps. Unlike the automatic per-scrape lookup, this fetches one
/// Twitch token up front and reuses it for the whole run instead of one
/// token exchange per game, and paces requests to stay under IGDB/Twitch's
/// rate limit (4 req/s on the free tier) instead of firing them all at once.
/// A game whose description/genre/developer/publisher/image are *all*
/// already set is skipped without an API call, so re-running this after an
/// earlier pass only costs requests for games still missing something.
#[tauri::command]
async fn igdb_backfill_all(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, db::DbState>,
    http: tauri::State<'_, reqwest::Client>,
) -> Result<String, String> {
    let settings = settings::load_settings();
    if settings.igdb_client_id.trim().is_empty() || settings.igdb_client_secret.trim().is_empty() {
        return Err("IGDB credentials aren't set -- add them in Settings first".into());
    }

    let games = {
        let conn = db_state.0.lock().map_err(|e| e.to_string())?;
        db::get_all_games(&conn).map_err(|e| e.to_string())?
    };
    let total = games.len() as u32;

    let token = igdb::get_token(&http, &settings.igdb_client_id, &settings.igdb_client_secret)
        .await
        .map_err(|e| format!("Couldn't authenticate with IGDB: {e}"))?;

    let mut processed = 0u32;
    let mut updated = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for game in games {
        processed += 1;

        let (Some(id), Some(title)) = (game.id, game.title.clone()) else {
            skipped += 1;
            continue;
        };

        // igdb_matched, not "are these columns non-NULL" -- RuTracker's own
        // post parsing and images.rs's Steam-search fallback already fill
        // description/genre/developer/publisher/image_url for nearly every
        // game, so a non-NULL check here used to skip almost the entire
        // library without ever actually calling IGDB.
        if game.igdb_matched != 0 {
            skipped += 1;
            continue;
        }

        let _ = app.emit(
            "igdb_backfill_progress",
            IgdbBackfillProgress {
                status: format!("Looking up {title}..."),
                processed,
                total,
                updated,
                skipped,
                failed,
            },
        );

        match igdb::fetch_metadata_with_token(&http, &token, &settings.igdb_client_id, &title).await {
            Ok(Some(meta)) => {
                let conn = db_state.0.lock().map_err(|e| e.to_string())?;
                let _ = db::apply_igdb_metadata(
                    &conn,
                    id,
                    meta.description.as_deref(),
                    meta.genre.as_deref(),
                    meta.developer.as_deref(),
                    meta.publisher.as_deref(),
                    meta.image_url.as_deref(),
                );
                updated += 1;
            }
            Ok(None) => skipped += 1,
            Err(e) => {
                failed += 1;
                eprintln!("[igdb] backfill lookup failed for '{title}': {e}");
            }
        }

        // Stay comfortably under IGDB/Twitch's free-tier rate limit (~4
        // requests/sec) instead of hammering it in a tight loop.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    let _ = app.emit(
        "igdb_backfill_progress",
        IgdbBackfillProgress {
            status: "Finished".into(),
            processed,
            total,
            updated,
            skipped,
            failed,
        },
    );

    Ok(format!("updated={updated} skipped={skipped} failed={failed} total={total}"))
}

/// Drives the already-logged-in main window through the ENTIRE Oculus Quests
/// tracker listing, page by page, parsing each row, fetching each new post's
/// detail page the same way, and parsing + translating + storing it. This is
/// the Rust/WebView equivalent of worker.ts's startScraper() -- it used to
/// stop after the first page's first 5 rows (a leftover from when this was a
/// one-off manual verification step), which meant both the manual "Scrape
/// now" button and the auto-scrape background loop only ever picked up a
/// handful of the newest listings instead of actually catching the library
/// up with the forum. PAGE_CAP is a hard backstop against a runaway loop if
/// has_next's parsing is ever wrong on a live page, not an expected
/// real-world ceiling.
#[tauri::command]
async fn start_scraper_test(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, db::DbState>,
    pending: tauri::State<'_, PendingFetches>,
    http: tauri::State<'_, reqwest::Client>,
    guards: tauri::State<'_, ScraperGuards>,
) -> Result<String, String> {
    let Some(_run_guard) = RunGuard::try_acquire(&guards.rutracker_running) else {
        return Ok("already running -- skipped".into());
    };

    const PAGE_SIZE: u32 = 50;
    const PAGE_CAP: u32 = 60;

    // Ported from the original VRSideForge Node backend's scraper
    // (backend/src/scraper/worker.ts, startScraper()), read directly rather
    // than guessed: that scraper never relied on a single tracker.php?f=2420
    // pagination walk. It ran FIVE independent queries against the same
    // forum and merged the results -- the plain tracker.php listing, a
    // completely different endpoint (viewforum.php, a topic-view page
    // parsed with different selectors, not a torrent listing), and three
    // more tracker.php searches narrowed by an `nm=` keyword (mirroring the
    // per-query result cap this app already found and worked around on
    // NNM-Club). This Rust port only ever implemented the first of those
    // five queries -- which is the real, concrete explanation for
    // "RuTracker only returns ~500 instead of 1600+": most of the original
    // app's coverage was never ported at all, not just a pagination bug on
    // the one query that was.
    struct RutrackerQuery {
        name: &'static str,
        base_url: &'static str,
        is_tracker: bool,
    }

    const RUTRACKER_QUERIES: [RutrackerQuery; 5] = [
        RutrackerQuery { name: "Oculus Quest games", base_url: "/forum/tracker.php?f=2420", is_tracker: true },
        RutrackerQuery { name: "Deep Archive (all Quest)", base_url: "/forum/viewforum.php?f=2420", is_tracker: false },
        RutrackerQuery { name: "Quest 3S", base_url: "/forum/tracker.php?f=2420&nm=Quest+3S", is_tracker: true },
        RutrackerQuery { name: "VR Meta Quest", base_url: "/forum/tracker.php?f=2420&nm=VR+Meta+Quest", is_tracker: true },
        RutrackerQuery { name: "Quest 3", base_url: "/forum/tracker.php?f=2420&nm=Quest+3", is_tracker: true },
    ];

    let _ = app.emit(
        "scraper_progress",
        ScraperProgress { status: "Fetching list page...".into(), saved: 0, skipped: 0, failed: 0 },
    );

    let mut saved = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for query in RUTRACKER_QUERIES {
    eprintln!("rutracker query '{}' ({}): starting", query.name, query.base_url);
    let sep = if query.base_url.contains('?') { "&" } else { "?" };

    for page in 0..PAGE_CAP {
    let start = page * PAGE_SIZE;
    let list_url = format!("{}{sep}start={start}", query.base_url);

    let list_outcome =
        fetch_via_webview(app.clone(), pending.clone(), list_url.clone(), None, None, None).await?;
    if list_outcome.status != 200 {
        return Err(format!("list page returned status {}", list_outcome.status));
    }

    let (items, has_next, auth_needed) = scrape::parse_list_page(&list_outcome.text, query.is_tracker);
    // Diagnostic -- this is the only way to tell, from the terminal log
    // alone, whether a run that stops earlier than expected hit a genuinely
    // empty/last page or something else (an auth/Cloudflare page that
    // doesn't trip auth_needed's own check, a malformed page, etc). If a
    // run reports fewer total items than the forum is believed to hold,
    // this line (search the log for "rutracker [") says exactly which
    // query/page it stopped at and why.
    eprintln!(
        "rutracker [{}] page {page} (start={start}): {} items, has_next={has_next}, auth_needed={auth_needed}",
        query.name,
        items.len()
    );
    if auth_needed {
        return Err("Session looks logged out -- log in in the window first".into());
    }
    // Loop control is driven by items.is_empty() (the page itself came back
    // with nothing), NOT has_next -- has_next depends on matching a "next
    // page" link's CSS class and Russian button text, which is brittle
    // against a real, possibly-redesigned live page and was never actually
    // verified against one (rutracker sits behind Cloudflare, so it can't
    // be checked anonymously). If has_next silently mismatches and always
    // comes back false, relying on it would silently cap this at a single
    // page every run -- exactly the "only ever grabs a handful" bug this
    // function is being fixed for. have_next is still used as a secondary
    // signal below (stop early when the page explicitly says there's no
    // more), but a page with actual rows always gets walked to the next
    // start= regardless.
    // An empty page after earlier pages had full 50-item pages is
    // suspicious, not reassuring -- rutracker sits behind Cloudflare, and
    // needs_auth() only recognizes an actual login form/captcha, not a
    // generic rate-limit or "please wait" interstitial, which would parse
    // as "0 rows, not logged out" and look exactly like a clean end of
    // listing. Rather than trust that on the first empty page, wait and
    // retry once before concluding the listing is actually over -- if the
    // retry comes back with real rows, this was a transient block/rate
    // limit, not the end.
    let mut items = items;
    if items.is_empty() && page > 0 {
        eprintln!(
            "rutracker [{}] page {page} (start={start}): came back empty right after pages that were              all full -- retrying once after a 5s backoff in case this was a rate limit/anti-bot              page rather than the real end of the listing",
            query.name
        );
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let retry_outcome =
            fetch_via_webview(app.clone(), pending.clone(), list_url.clone(), None, None, None).await?;
        if retry_outcome.status == 200 {
            let (retry_items, retry_has_next, retry_auth_needed) =
                scrape::parse_list_page(&retry_outcome.text, query.is_tracker);
            eprintln!(
                "rutracker [{}] page {page} retry (start={start}): {} items, has_next={retry_has_next}, auth_needed={retry_auth_needed}",
                query.name,
                retry_items.len()
            );
            if retry_auth_needed {
                return Err("Session looks logged out -- log in in the window first".into());
            }
            items = retry_items;
        }
    }

    let page_had_items = !items.is_empty();
    if !page_had_items {
        break;
    }

    // A short gap between consecutive list-page requests -- the previous
    // run hit exactly 10 full pages back-to-back before coming back empty,
    // which is consistent with tripping a request-rate limit rather than
    // genuinely running out of content. This alone won't prove or disprove
    // that, but it costs nothing and reduces the odds of causing it.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    for item in items {
        let already_have = {
            let conn = db_state.0.lock().map_err(|e| e.to_string())?;
            db::game_exists(&conn, &item.link).map_err(|e| e.to_string())?
        };
        if already_have {
            skipped += 1;
            continue;
        }

        let _ = app.emit(
            "scraper_progress",
            ScraperProgress {
                status: format!("Fetching {}...", item.link),
                saved,
                skipped,
                failed,
            },
        );

        // fetch_via_webview expects a path relative to the window's current origin,
        // same as parse_list_page's output being an absolute rutracker.org URL --
        // strip the origin back off since the window is already there.
        let path = item
            .link
            .strip_prefix("https://rutracker.org")
            .unwrap_or(&item.link)
            .to_string();

        let detail = match fetch_via_webview(app.clone(), pending.clone(), path, None, None, None).await {
            Ok(o) if o.status == 200 => o,
            Ok(o) => {
                failed += 1;
                db_state
                    .0
                    .lock()
                    .map_err(|e| e.to_string())?
                    .execute("SELECT 1", [])
                    .ok(); // no-op, keeps lock scoping obvious
                let conn = db_state.0.lock().map_err(|e| e.to_string())?;
                let _ = db::record_failed_game(&conn, &item.link, &item.size, &format!("HTTP {}", o.status));
                continue;
            }
            Err(e) => {
                failed += 1;
                let conn = db_state.0.lock().map_err(|e| e.to_string())?;
                let _ = db::record_failed_game(&conn, &item.link, &item.size, &e);
                continue;
            }
        };

        let parsed = match scrape::parse_post_details(&detail.text, &item.link) {
            Some(p) => p,
            None => {
                failed += 1;
                let conn = db_state.0.lock().map_err(|e| e.to_string())?;
                let _ = db::record_failed_game(&conn, &item.link, &item.size, "Could not parse post (session expired?)");
                continue;
            }
        };

        let image_url = images::fetch_game_image(&http, &parsed.title, parsed.post_image.as_deref()).await;
        // Target language comes from the user's own Settings (default "en") --
        // this used to be hardcoded to "pt", which silently ignored whatever
        // the user actually configured.
        let target_lang = settings::load_settings().translation_language;
        let translated_title = translate::translate_text(&http, &parsed.title, &target_lang).await;
        let translated_description = translate::translate_text(&http, &parsed.description, &target_lang).await;

        let game = db::Game {
            id: None,
            title: Some(parsed.title.clone()),
            translated_title: Some(translated_title),
            description: Some(parsed.description.clone()),
            translated_description: Some(translated_description),
            magnet: Some(parsed.magnet.clone()),
            post_url: Some(parsed.post_url.clone()),
            tags: Some(parsed.tags.clone()),
            size: if parsed.size != "Unknown" { Some(parsed.size.clone()) } else { Some(item.size.clone()) },
            image_url,
            seeds: parsed.seeds,
            leeches: parsed.leeches,
            registered_at: Some(parsed.registered_at.clone()),
            torrent_downloads: parsed.torrent_downloads,
            wishlist: 0,
            genre: None,
            developer: None,
            publisher: None,
            version: None,
            languages: None,
            play_modes: None,
            igdb_matched: 0,
        };

        let new_id = {
            let conn = db_state.0.lock().map_err(|e| e.to_string())?;
            db::insert_game(&conn, &game).map_err(|e| e.to_string())?
        };
        try_fetch_igdb_metadata(&http, &db_state, new_id, &parsed.title).await;
        saved += 1;

        let _ = app.emit(
            "scraper_progress",
            ScraperProgress { status: format!("Saved: {}", parsed.title), saved, skipped, failed },
        );
    }

    // Deliberately NOT stopping on `!has_next` here -- see the comment
    // above where page_had_items is computed. The only stop condition is
    // the next page coming back empty (handled at the top of the next
    // iteration) or PAGE_CAP. `has_next` is parsed but otherwise unused
    // now; kept in the tuple destructure so it's still there to wire back
    // in if a real has_next bug ever turns out NOT to be the cause.
    let _ = has_next;
    }
    }

    let _ = app.emit(
        "scraper_progress",
        ScraperProgress { status: "Finished".into(), saved, skipped, failed },
    );

    Ok(format!("saved={saved} skipped={skipped} failed={failed}"))
}

/// Equivalent manual verification step for nnmclub (see start_scraper_test's
/// doc comment above) -- scoped to one search keyword against forum 1024
/// ("\u0418\u0433\u0440\u044b \u0434\u043b\u044f VR" / VR Games), using
/// its own "nnmclub" window. Unlike rutracker's flow, nnmclub's list row
/// often already carries a usable magnet link directly, so this skips the
/// separate detail-page fetch entirely for rows that have one. A row whose
/// only link is download.php (a .torrent file, not a magnet): that file is
/// fetched directly with the app's own `http` (reqwest) client -- not the
/// nnmclub webview, see the long comment at the fetch call site below for
/// why -- and turned into a magnet URI from its info-hash
/// (`torrentfile::to_magnet`) -- `unresolved` now only counts an actual
/// fetch/parse failure, not "this one needed resolving".
#[tauri::command]
async fn start_nnmclub_scraper_test(
    app: tauri::AppHandle,
    db_state: tauri::State<'_, db::DbState>,
    pending: tauri::State<'_, PendingFetches>,
    http: tauri::State<'_, reqwest::Client>,
    guards: tauri::State<'_, ScraperGuards>,
    keyword: Option<String>,
) -> Result<String, String> {
    let Some(_run_guard) = RunGuard::try_acquire(&guards.nnmclub_running) else {
        return Ok("already running -- skipped".into());
    };

    let keyword = keyword.unwrap_or_else(|| "VR".to_string());
    // nnmclub's search expects windows-1251-encoded, percent-escaped query
    // text (per the reference scraper). For plain ASCII keywords ("VR",
    // "Oculus", "Oculus Quest") that's identical to a plain percent-encode,
    // so this does a simple byte-wise percent-encode rather than pulling in
    // an encoding crate just for this. A Cyrillic keyword would need real
    // windows-1251 encoding first -- not handled here.
    let encoded_kw: String = keyword
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    // NNM-Club's search box is a POST form (action="/forum/tracker.php",
    // method="post") -- verified directly against the live site. A GET
    // request with the same nm= as a query string is silently ignored
    // (tracker.php only reads $_POST here), which is why this used to come
    // back saved=0 skipped=0 unresolved=0 even for titles that are
    // definitely on the site: it was never actually searching anything.
    //
    // f=-1 ("search all forums") looked like the right default, but a
    // live search with it hits a hard server-side cap that has nothing to
    // do with pagination: the results page itself says "Результатов
    // поиска: 50 (max: 50)" ("Search results: 50 (max: 50)") -- NNM-Club
    // simply refuses to return more than 50 rows for any ONE query,
    // confirmed directly against the live site, not guessed. There's no
    // `start=` or "next page" to walk past that; "Страница 1 из 1" (page 1
    // of 1) on that same results page confirms it's not paginated, it's
    // just truncated. That's the real reason a keyword search here always
    // seemed to top out around 50 regardless of how many VR titles
    // actually exist on the site.
    //
    // The cap is per-QUERY, not global (confirmed live: narrowing f=-1 to
    // a single forum ID returns a smaller, uncapped-for-that-forum count
    // every time), and NNM-Club has no dedicated "VR Games" forum at all
    // -- f=1024 (what this used to search) is actually "macOS
    // (Apple/Hackintosh)", unrelated; real VR titles here are tagged
    // "[VR]" in the title and scattered across the normal PC-game genre
    // forums. So instead of one global search that's capped at 50 total,
    // this runs the same keyword search once per PC-game forum under
    // "Форум Игры" (ids confirmed live via viewforum.php?f=36) and
    // combines the results -- each forum gets its own 50-row ceiling, and
    // in practice no single genre forum has anywhere near 50 VR-tagged
    // titles, so the combined total isn't artificially capped anymore.
    const GAME_FORUM_IDS: [&str; 13] = [
        "412",  // Action (FPS)
        "1008", // Action (TPS)
        "415",  // Adventure/Quest
        "746",  // Arcade
        "428",  // Online (MMO)
        "413",  // RPG
        "414",  // Strategy (RTS/TBS/Grand)
        "1010", // Strategy Tactical (RTS/TBS)
        "1014", // Racing
        "416",  // Simulation (Flight/Space)
        "1013", // Simulation (Sport)
        "268",  // Action/Arcade/Platformer (Casual)
        "1017", // Strategy/Manager/Business (Casual)
    ];

    let _ = app.emit(
        "scraper_progress",
        ScraperProgress { status: "Fetching nnmclub list pages...".into(), saved: 0, skipped: 0, failed: 0 },
    );

    let mut saved = 0u32;
    let mut skipped = 0u32;
    let mut unresolved = 0u32;

    for forum_id in GAME_FORUM_IDS {
    let list_url = "/forum/tracker.php".to_string();
    let post_body = format!("f={forum_id}&nm={encoded_kw}");

    let list_outcome = fetch_via_webview(
        app.clone(),
        pending.clone(),
        list_url,
        Some("nnmclub".to_string()),
        Some("POST".to_string()),
        Some(post_body),
    )
    .await?;
    if list_outcome.status != 200 {
        return Err(format!("nnmclub list page returned status {}", list_outcome.status));
    }

    let items = scrape::parse_nnmclub_list_page(&list_outcome.text);
    eprintln!("nnmclub forum f={forum_id}: {} items", items.len());

    // A topic can in principle be cross-posted/matched from more than one
    // forum scope; game_exists() below (keyed on topic_url) already skips
    // anything already saved, so no extra de-dup is needed across the
    // forum_id loop iterations.
    for item in items.into_iter() {
        let already_have = {
            let conn = db_state.0.lock().map_err(|e| e.to_string())?;
            db::game_exists(&conn, &item.topic_url).map_err(|e| e.to_string())?
        };
        if already_have {
            skipped += 1;
            continue;
        }

        // NNM-Club's rows don't always carry a ready-made magnet: URI --
        // some only expose a download.php link (the raw .torrent file).
        // That link 302-redirects to a DIFFERENT subdomain
        // (bulk.nnmclub.to) which sends no CORS headers at all -- fetching
        // it from the nnmclub webview's own JS (fetch_binary_via_webview)
        // hit exactly the cross-origin-redirect case where WebKit's
        // fetch() is known to hang instead of cleanly rejecting, rather
        // than ever actually timing out on anything slow. Confirmed via a
        // plain curl: both hops resolve in well under a second, and the
        // final file comes back with no login/cookies needed at all --
        // so this is done with the app's own `http` (reqwest) client
        // instead, which has no CORS concept and already follows
        // redirects by default, exactly like `curl -L` would.
        let magnet = if item.download_url.starts_with("magnet:") {
            Some(item.download_url.clone())
        } else if item.download_url.is_empty() {
            None
        } else {
            match http.get(&item.download_url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                    Ok(bytes) => torrentfile::to_magnet(&bytes, &item.title),
                    Err(e) => {
                        eprintln!(
                            "nnmclub: couldn't read .torrent file body for '{}' ({}): {e}",
                            item.title, item.download_url
                        );
                        None
                    }
                },
                Ok(resp) => {
                    eprintln!(
                        "nnmclub: fetching .torrent file for '{}' ({}) returned status {}",
                        item.title, item.download_url, resp.status()
                    );
                    None
                }
                Err(e) => {
                    eprintln!(
                        "nnmclub: fetching .torrent file for '{}' ({}) failed: {e}",
                        item.title, item.download_url
                    );
                    None
                }
            }
        };

        let Some(magnet) = magnet else {
            unresolved += 1;
            continue;
        };

        let game = db::Game {
            id: None,
            title: Some(item.title.clone()),
            translated_title: None,
            description: None,
            translated_description: None,
            magnet: Some(magnet),
            post_url: Some(item.topic_url.clone()),
            // NNM-Club is a PC-game tracker -- every listing on it is a
            // Windows build meant to be run tethered to a PC's GPU, never
            // a standalone-headset build, confirmed via the genre forums
            // this loop reads from and the "License"/"Repack"/"Scene" PC
            // release-group conventions in these titles. Tagged "PCVR"
            // (rather than the generic "VR" every title already carries
            // in its own trailing "[VR]" bracket) so the Library's
            // platform filter and tag chip can actually distinguish these
            // from RuTracker's standalone Quest builds, which get their
            // own device-specific tags elsewhere in this file.
            tags: Some("PCVR".to_string()),
            size: Some(item.size.clone()),
            image_url: None,
            seeds: item.seeds,
            leeches: 0,
            registered_at: None,
            torrent_downloads: 0,
            wishlist: 0,
            genre: None,
            developer: None,
            publisher: None,
            version: None,
            languages: None,
            play_modes: None,
            igdb_matched: 0,
        };

        let new_id = {
            let conn = db_state.0.lock().map_err(|e| e.to_string())?;
            db::insert_game(&conn, &game).map_err(|e| e.to_string())?
        };
        try_fetch_igdb_metadata(&http, &db_state, new_id, &item.title).await;
        saved += 1;

        let _ = app.emit(
            "scraper_progress",
            ScraperProgress { status: format!("Saved: {}", item.title), saved, skipped, failed: unresolved },
        );

        // RuTracker's per-item loop naturally paces itself (it fetches a
        // full detail page, an image, and runs a translation per item
        // before ever calling IGDB). This loop only fetches a .torrent file
        // -- much faster -- and, unlike RuTracker, calls IGDB for every new
        // item in a tight burst with no cached token (see
        // try_fetch_igdb_metadata/get_token: every single call re-does the
        // full Twitch OAuth exchange, there's no caching at all). A short
        // delay here is a best-effort guard against tripping Twitch's/
        // IGDB's rate limiting on a run that finds many new NNM-Club
        // titles back to back -- not a confirmed fix, since a rate-limit
        // error would only show up as a "[igdb] metadata lookup failed"
        // line in the terminal log, which hasn't been checked yet.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    }

    let _ = app.emit(
        "scraper_progress",
        ScraperProgress { status: "Finished".into(), saved, skipped, failed: unresolved },
    );

    let note = if unresolved > 0 {
        " (unresolved = .torrent file fetch or parse failed -- see the terminal log for which ones and why)"
    } else {
        ""
    };
    Ok(format!("saved={saved} skipped={skipped} unresolved={unresolved}{note}"))
}

/// Brings one source's login/scraping window to the front so the user can
/// sign in (or just check on it), in response to clicking that source's
/// "Log in" button in Settings. Only ever acts on a window label one of
/// sources.rs's own definitions names -- never an arbitrary caller-supplied
/// label -- so this can't be used to show/focus some other window by guessing
/// its name.
#[tauri::command]
fn open_source_login(app: tauri::AppHandle, window_label: String) -> Result<(), String> {
    let known = sources::WINDOWED.iter().any(|s| s.window_label == window_label);
    if !known {
        return Err(format!("'{window_label}' is not a known torrent source window"));
    }
    let window = app
        .get_webview_window(&window_label)
        .ok_or_else(|| format!("{window_label} window not found"))?;
    // The window is created hidden (see run()'s setup) and stays that way
    // until the user explicitly asks to log in -- this is that ask.
    window
        .set_position(tauri::PhysicalPosition::new(100, 100))
        .map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

/// Checks whether the user is actually logged in to a source, by fetching a
/// same-origin page through that source's OWN window (so it carries real
/// cookies) and running `scrape::page_needs_auth` on the result. Returns
/// `Ok(true)` when logged in, `Ok(false)` when not (or the page couldn't be
/// fetched), `Err` only for a genuinely unknown window label or a source
/// with no check implemented (the Server source).
#[tauri::command]
async fn check_source_login(
    app: tauri::AppHandle,
    pending: tauri::State<'_, PendingFetches>,
    window_label: String,
) -> Result<bool, String> {
    let source = sources::WINDOWED
        .iter()
        .find(|s| s.window_label == window_label)
        .ok_or_else(|| format!("'{window_label}' is not a known torrent source window"))?;

    if source.check_path.is_empty() {
        return Err(format!("no login check implemented for {}", source.display_name));
    }

    let outcome = fetch_via_webview(
        app,
        pending,
        source.check_path.to_string(),
        Some(window_label),
        None,
        None,
    )
    .await?;

    if outcome.status != 200 {
        return Ok(false);
    }

    Ok(!scrape::page_needs_auth(&outcome.text))
}

// ---- Games CRUD (port of /api/games/:id GET/PUT/DELETE, /api/games/:id/wishlist) ----

#[tauri::command]
fn get_game(db: tauri::State<db::DbState>, id: i64) -> Result<Option<db::Game>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    db::get_game_by_id(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_game(
    db: tauri::State<db::DbState>,
    id: i64,
    title: Option<String>,
    tags: Option<String>,
    genre: Option<String>,
    developer: Option<String>,
    publisher: Option<String>,
    version: Option<String>,
    languages: Option<String>,
    play_modes: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    db::update_game_fields(
        &conn,
        id,
        title.as_deref(),
        tags.as_deref(),
        genre.as_deref(),
        developer.as_deref(),
        publisher.as_deref(),
        version.as_deref(),
        languages.as_deref(),
        play_modes.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_game(db: tauri::State<db::DbState>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    db::delete_game(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_wishlist(db: tauri::State<db::DbState>, id: i64, wishlist: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    db::set_wishlist(&conn, id, wishlist).map_err(|e| e.to_string())
}

// ---- Settings (port of GET/POST /api/settings) ----

#[tauri::command]
fn get_settings() -> settings::Settings {
    settings::load_settings()
}

#[tauri::command]
fn save_settings(partial: serde_json::Value) -> Result<settings::Settings, String> {
    settings::save_settings(&partial).map_err(|e| e.to_string())
}

// ---- Session (port of /api/session/status, /validate, /import-cookies) ----

#[tauri::command]
fn session_exists(db: tauri::State<db::DbState>) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    Ok(db::get_session(&conn).map_err(|e| e.to_string())?.is_some())
}

#[tauri::command]
async fn session_check(
    db: tauri::State<'_, db::DbState>,
    http: tauri::State<'_, reqwest::Client>,
) -> Result<session::SessionCheck, String> {
    let stored = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        db::get_session(&conn).map_err(|e| e.to_string())?
    }; // lock dropped here, before the .await below
    Ok(session::check_stored_session(&http, stored).await)
}

#[tauri::command]
async fn import_cookies(
    db: tauri::State<'_, db::DbState>,
    http: tauri::State<'_, reqwest::Client>,
    bb_session: String,
    cf_clearance: Option<String>,
    user_agent: Option<String>,
) -> Result<session::SessionCheck, String> {
    let cookies_json = session::build_cookies_json(&bb_session, cf_clearance.as_deref());
    let stored = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        db::save_session(&conn, &cookies_json, user_agent.as_deref()).map_err(|e| e.to_string())?;
        db::get_session(&conn).map_err(|e| e.to_string())?
    }; // lock dropped here, before the .await below
    Ok(session::check_stored_session(&http, stored).await)
}

// ---- qBittorrent (port of /api/torrent/*) ----

#[tauri::command]
async fn qbit_check(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
) -> Result<qbittorrent::TorrentCheck, String> {
    Ok(qbittorrent::check(&http, &qbit).await)
}

#[tauri::command]
async fn qbit_add_torrent(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
    magnet: String,
    savepath: String,
) -> Result<(), String> {
    qbittorrent::add_torrent(&http, &qbit, &magnet, &savepath).await
}

#[tauri::command]
async fn qbit_list_torrents(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
) -> Result<Vec<qbittorrent::TorrentInfo>, String> {
    qbittorrent::list_torrents(&http, &qbit).await
}

#[tauri::command]
async fn qbit_torrent_action(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
    hash: String,
    action: String,
) -> Result<(), String> {
    qbittorrent::torrent_action(&http, &qbit, &hash, &action).await
}

// ---- Hybrid torrent engine (rqbit primary, qBittorrent automatic fallback) ----
// Engine-agnostic commands the frontend should actually use; the qbit_* commands
// above stay as direct qBittorrent access for settings/diagnostics display.

#[tauri::command]
async fn torrent_add(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
    engine: tauri::State<'_, torrent::TorrentEngineState>,
    magnet: String,
    savepath: String,
) -> Result<torrent::TorrentInfo, String> {
    torrent::add_torrent(&engine, &http, &qbit, &magnet, &savepath).await
}

#[tauri::command]
async fn torrent_list(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
    engine: tauri::State<'_, torrent::TorrentEngineState>,
) -> Result<Vec<torrent::TorrentInfo>, String> {
    Ok(torrent::list_torrents(&engine, &http, &qbit).await)
}

#[tauri::command]
async fn torrent_action(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
    engine: tauri::State<'_, torrent::TorrentEngineState>,
    hash: String,
    action: String,
) -> Result<(), String> {
    torrent::torrent_action(&engine, &http, &qbit, &hash, &action).await
}

// ---- ADB / sideload (port of /api/adb/*) ----

#[tauri::command]
async fn adb_devices() -> Vec<String> {
    adb::get_adb_devices().await
}

#[tauri::command]
async fn adb_check_path() -> bool {
    adb::check_adb_path().await
}

#[derive(serde::Serialize)]
struct AdbAppsResult {
    apps: Vec<String>,
    storage: Option<adb::StorageInfo>,
}

#[tauri::command]
async fn adb_apps(device_id: Option<String>) -> AdbAppsResult {
    let (apps, storage) = tokio::join!(
        adb::get_installed_apps(device_id.as_deref()),
        adb::get_storage_info(device_id.as_deref())
    );
    AdbAppsResult { apps, storage }
}

#[tauri::command]
async fn adb_uninstall(pkg: String, device_id: Option<String>) -> bool {
    adb::uninstall_app(&pkg, device_id.as_deref()).await
}

/// Manual, on-demand local backup of one installed app's APK + OBB data,
/// into the user's configured backup folder. Never triggered automatically --
/// only by this command, called in direct response to a user button click.
#[tauri::command]
async fn adb_backup_app(pkg: String, device_id: Option<String>) -> Result<adb::BackupResult, String> {
    adb::backup_app(&pkg, device_id.as_deref(), &settings::load_settings().backup_path).await
}

/// Port of `POST /api/adb/install`: kicks off the SideForge sideload
/// pipeline. Unlike the Node route (which responds immediately and runs the
/// install in the background, reporting progress over socket.io), this one
/// runs to completion and returns the result directly -- the frontend gets
/// its "it started" signal from the promise resolving pending state, and
/// live progress from the "adb_event" events sideload.rs emits as it runs,
/// same event name/shape the original used.
#[tauri::command]
async fn adb_install(
    app: tauri::AppHandle,
    folder_path: String,
    device_id: Option<String>,
    lang: Option<String>,
) -> sideload::SideloadResult {
    sideload::perform_sideforge_sideload(&app, &folder_path, device_id.as_deref(), lang.as_deref().unwrap_or("en")).await
}

// ---- Filesystem scan (port of /api/filesystem/*) ----
// All four operate on the user's configured download folder, matching the
// Node version's globalDownloadPath.

#[tauri::command]
async fn fs_folders(
    http: tauri::State<'_, reqwest::Client>,
    qbit: tauri::State<'_, qbittorrent::QbitState>,
    engine: tauri::State<'_, torrent::TorrentEngineState>,
) -> Result<Vec<filesystem::FolderInfo>, String> {
    // Cross-reference the torrent engine's live view so a folder whose
    // download is still in progress doesn't show up as ready to install
    // just because its .apk file already exists on disk (see
    // FolderInfo::download_progress's doc comment in filesystem.rs).
    //
    // Note: an async command that takes State (a reference-ish input) has
    // to return a Result, not a bare value -- Tauri's macro needs that to
    // produce a 'static future (see AsyncCommandMustReturnResult). This
    // can't actually fail -- list_torrents() already swallows its own
    // errors -- so there's no real failure path, just the Ok(...) wrapper.
    let active_torrents: std::collections::HashMap<String, f64> = torrent::list_torrents(&engine, &http, &qbit)
        .await
        .into_iter()
        .filter(|t| t.progress < 100.0)
        .map(|t| (t.name, t.progress))
        .collect();

    Ok(filesystem::folders(&settings::load_settings().download_path, &active_torrents))
}

#[tauri::command]
fn fs_scan_local_downloads(db: tauri::State<db::DbState>) -> Result<filesystem::ScanResult, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    filesystem::scan_local_downloads(&conn, &settings::load_settings().download_path)
}

#[tauri::command]
fn fs_manual_index(folder_name: String, game_id: i64) -> Result<(), String> {
    filesystem::manual_index(&settings::load_settings().download_path, &folder_name, game_id)
}

#[tauri::command]
fn fs_remove_index(folder_name: String) -> Result<bool, String> {
    filesystem::remove_index(&settings::load_settings().download_path, &folder_name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Two independent background loops -- one per source -- each sleeping
/// `AUTO_SCRAPE_INTERVAL` between attempts and re-checking Settings every
/// time it wakes up, so toggling "Auto-scrape" on/off in Settings takes
/// effect on the very next tick without needing a restart. Spawned once
/// from `run()`'s `setup()`, after the source windows exist (these call
/// straight into the same `start_scraper_test`/`start_nnmclub_scraper_test`
/// commands the "Scrape now" buttons use -- a Tauri command is just a
/// plain async fn, nothing stops calling it directly like this instead of
/// through IPC). A failure (not logged in, site unreachable, etc.) is
/// logged and the loop just tries again next interval -- same
/// never-crash-the-app posture as the rest of the scraping code.
fn spawn_auto_scrape_loops(app: tauri::AppHandle) {
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(AUTO_SCRAPE_INITIAL_DELAY).await;
            loop {
                if settings::load_settings().auto_scrape_rutracker {
                    let db_state = app.state::<db::DbState>();
                    let pending = app.state::<PendingFetches>();
                    let http = app.state::<reqwest::Client>();
                    let guards = app.state::<ScraperGuards>();
                    match start_scraper_test(app.clone(), db_state, pending, http, guards).await {
                        Ok(result) => eprintln!("auto-scrape (rutracker): {result}"),
                        Err(e) => eprintln!("auto-scrape (rutracker) skipped/failed: {e}"),
                    }
                }
                tokio::time::sleep(AUTO_SCRAPE_INTERVAL).await;
            }
        });
    }

    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(AUTO_SCRAPE_INITIAL_DELAY).await;
            loop {
                if settings::load_settings().auto_scrape_nnmclub {
                    let db_state = app.state::<db::DbState>();
                    let pending = app.state::<PendingFetches>();
                    let http = app.state::<reqwest::Client>();
                    let guards = app.state::<ScraperGuards>();
                    match start_nnmclub_scraper_test(app.clone(), db_state, pending, http, guards, None).await {
                        Ok(result) => eprintln!("auto-scrape (nnmclub): {result}"),
                        Err(e) => eprintln!("auto-scrape (nnmclub) skipped/failed: {e}"),
                    }
                }
                tokio::time::sleep(AUTO_SCRAPE_INTERVAL).await;
            }
        });
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(PendingFetches::default())
        .manage(ScraperGuards::default())
        .manage(PendingBinaryFetches::default())
        .manage(reqwest::Client::new())
        .manage(qbittorrent::QbitState::default())
        .manage(torrent::TorrentEngineState::default())
        .setup(|app| {
            let conn = db::init_db().expect("failed to open/initialize database");
            app.manage(db::DbState(Mutex::new(conn)));

            // One window per indexed torrent source (sources.rs), each
            // independently navigated and logged in, so all three can stay
            // authenticated at once. "main" (rutracker) keeps its original
            // behavior unchanged; nnmclub and the Server source are new.
            for source in sources::WINDOWED {
                let url = tauri::Url::parse(source.start_url)
                    .unwrap_or_else(|e| panic!("invalid start URL for {}: {e}", source.display_name));

                // Tauri's default on a close click is to DESTROY the
                // window, not hide it -- so without this, closing a source
                // window after logging in means open_source_login can never
                // find it again (get_webview_window returns None). These
                // windows are meant to be long-lived background sessions, so
                // a close click hides instead; nothing actually destroys one
                // once created.
                //
                // Created hidden (`.visible(false)`). An earlier version of
                // this kept these windows visible-but-parked-off-screen
                // instead, on the theory that a genuinely hidden WKWebView's
                // content process gets suspended by WebKit and would hang
                // fetch() calls -- that theory was tested directly (rebuilt
                // with the window visible off-screen) and DIDN'T change the
                // actual bug's behavior at all, so it wasn't the real cause;
                // the real fix for that bug was routing the one binary
                // (.torrent file) fetch through the app's own reqwest client
                // instead of the window's JS, which is unrelated to
                // visibility. Meanwhile the visible-off-screen windows were
                // visibly popping up on launch for the user (off-screen
                // coordinates aren't reliably off-screen on every display
                // setup) with no actual upside, so this reverts to plainly
                // hidden, shown only on open_source_login's explicit request.
                let source_window = WebviewWindowBuilder::new(
                    app,
                    source.window_label,
                    WebviewUrl::External(url),
                )
                .title(format!(
                    "RooQuest -- log in to {} in this window",
                    source.display_name
                ))
                .inner_size(1200.0, 850.0)
                .initialization_script(KEEPALIVE_JS)
                .visible(false)
                .build()?;

                // on_window_event lives on the built WebviewWindow instance
                // in this Tauri version, not on the builder -- the builder
                // has no such method (its close/open-related hooks are
                // on_page_load/on_navigation/on_download, not window-level
                // lifecycle events). Clone the cheap window handle so the
                // closure can hide it instead of letting Tauri destroy it.
                let hide_handle = source_window.clone();
                source_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = hide_handle.hide();
                    }
                });
            }

            // The actual visible app window -- loads our own bundled
            // frontend (src/index.html, frontendDist in tauri.conf.json),
            // NOT an external site. The three source windows above are all
            // WebviewUrl::External and all hidden now, so without this one
            // there is nothing on screen at all: this is the window the user
            // actually sees and clicks things in.
            WebviewWindowBuilder::new(app, "app", WebviewUrl::App("index.html".into()))
                .title("RooQuest")
                .inner_size(720.0, 860.0)
                .visible(true)
                .build()?;

            spawn_auto_scrape_loops(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_games,
            start_scraper_test,
            fetch_via_webview,
            fetch_binary_via_webview,
            webview_fetch_binary_result,
            webview_fetch_result,
            get_game,
            update_game,
            delete_game,
            set_wishlist,
            get_settings,
            save_settings,
            session_exists,
            session_check,
            import_cookies,
            qbit_check,
            qbit_add_torrent,
            qbit_list_torrents,
            qbit_torrent_action,
            torrent_add,
            torrent_list,
            torrent_action,
            adb_devices,
            adb_check_path,
            adb_apps,
            adb_uninstall,
            adb_install,
            adb_backup_app,
            start_nnmclub_scraper_test,
            igdb_backfill_all,
            translate_backfill_all,
            translate_one,
            open_source_login,
            check_source_login,
            fs_folders,
            fs_scan_local_downloads,
            fs_manual_index,
            fs_remove_index,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
