//! Port of backend/src/index.ts's session-validity check (`checkStoredSession`)
//! and the cookie-import route. Unlike the WebView-driven scraper, this talks
//! to rutracker.org with a plain `reqwest` GET carrying the stored cookie
//! header -- same approach the Node backend used (a plain `fetch`, not a
//! headless browser), since this is just a quick "is this cookie still good"
//! probe, not a scrape that needs to survive a Cloudflare challenge page.

use serde::{Deserialize, Serialize};

const DEFAULT_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionCheck {
    pub ok: bool,
    pub challenged: bool,
    pub status: u16,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct StoredCookie {
    name: String,
    value: String,
}

/// Port of `checkStoredSession()`. Takes an already-fetched `StoredSession`
/// (owned, not a DB connection) so the caller can drop its DB lock before
/// awaiting this -- rusqlite's `Connection` isn't `Send`/`Sync`, so it can't
/// be held across an `.await` inside a Tauri async command.
pub async fn check_stored_session(client: &reqwest::Client, session: Option<crate::db::StoredSession>) -> SessionCheck {
    let Some(session) = session else {
        return SessionCheck { ok: false, challenged: false, status: 0 };
    };
    let Some(cookies_json) = session.cookies else {
        return SessionCheck { ok: false, challenged: false, status: 0 };
    };

    let cookies: Vec<StoredCookie> = match serde_json::from_str(&cookies_json) {
        Ok(c) => c,
        Err(_) => return SessionCheck { ok: false, challenged: false, status: 0 },
    };
    let cookie_header = cookies
        .iter()
        .map(|c| format!("{}={}", c.name, c.value))
        .collect::<Vec<_>>()
        .join("; ");
    let ua = session.user_agent.unwrap_or_else(|| DEFAULT_UA.to_string());

    let resp = client
        .get("https://rutracker.org/forum/index.php")
        .header("Cookie", cookie_header)
        .header("User-Agent", ua)
        .send()
        .await;

    let Ok(resp) = resp else {
        return SessionCheck { ok: false, challenged: false, status: 0 };
    };
    let status = resp.status().as_u16();
    let html = resp.text().await.unwrap_or_default();

    let challenged = html.contains("Just a moment") || status == 403 || status == 503;
    let ok = !challenged
        && !html.contains("name=\"login_username\"")
        && (html.contains("logged-in-username") || html.contains("Выход") || html.to_lowercase().contains("logout"));

    SessionCheck { ok, challenged, status }
}

/// Port of `POST /api/session/import-cookies`'s cookie-building + save step.
/// The verification call (`checkStoredSession` right after saving) is left to
/// the caller (the Tauri command), same as the Node route does.
pub fn build_cookies_json(bb_session: &str, cf_clearance: Option<&str>) -> String {
    let mut cookies = vec![StoredCookie {
        name: "bb_session".to_string(),
        value: bb_session.to_string(),
    }];
    if let Some(cf) = cf_clearance {
        if !cf.is_empty() {
            cookies.push(StoredCookie {
                name: "cf_clearance".to_string(),
                value: cf.to_string(),
            });
        }
    }
    serde_json::to_string(&cookies).unwrap_or_else(|_| "[]".to_string())
}
