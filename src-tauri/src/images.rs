//! Port of backend/src/utils/images.ts -- Steam store search for a cover image,
//! used when the forum post itself had none. Plain, unauthenticated HTTP is fine
//! here (Steam's search page isn't Cloudflare-gated like rutracker), so this one
//! stays on reqwest rather than the WebView.

use regex::Regex;
use scraper::{Html, Selector};
use std::sync::OnceLock;

pub async fn fetch_game_image(client: &reqwest::Client, full_title: &str, post_image_url: Option<&str>) -> Option<String> {
    if let Some(url) = post_image_url {
        if url.starts_with("http") {
            return Some(url.to_string());
        }
        if url.starts_with("//") {
            return Some(format!("https:{}", url));
        }
    }

    static BRACKET_RE: OnceLock<Regex> = OnceLock::new();
    let bracket_re = BRACKET_RE.get_or_init(|| Regex::new(r"\[.*?\]").unwrap());
    let clean_title = bracket_re.replace_all(full_title, "").trim().to_string();
    if clean_title.is_empty() {
        return None;
    }

    let search_url = format!(
        "https://store.steampowered.com/search/?term={}",
        urlencoding::encode(&clean_title)
    );

    let body = client.get(&search_url).send().await.ok()?.text().await.ok()?;
    let html = Html::parse_document(&body);
    let selector = Selector::parse("#search_resultsRows a:first-child .search_capsule img").ok()?;

    html.select(&selector)
        .next()
        .and_then(|el| el.value().attr("src"))
        .map(|src| src.split('?').next().unwrap_or(src).to_string())
}
