//! Port of backend/src/scraper/httpFetcher.ts's `fetchPostDetailsHttp` (field
//! extraction) plus worker.ts's list-page row extraction. The HTML passed in
//! here comes from webview_fetch.rs -- already fetched by the real WebView
//! engine, already decoded to a UTF-8 string by the browser's own fetch().text().

use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ParsedPost {
    pub title: String,
    pub description: String,
    pub magnet: String,
    pub post_url: String,
    pub tags: String,
    pub size: String,
    pub post_image: Option<String>,
    pub seeds: i64,
    pub leeches: i64,
    pub registered_at: String,
    pub torrent_downloads: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ListItem {
    pub link: String,
    pub size: String,
}

fn sel(css: &str) -> Selector {
    Selector::parse(css).expect("bad CSS selector")
}

fn text_of(html: &Html, css: &str) -> String {
    let selector = sel(css);
    html.select(&selector)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_default()
}

fn is_valid_image(src: &str) -> bool {
    if src.is_empty() {
        return false;
    }
    if src.contains("rutracker.cc/smiles") || src.contains("rutracker.cc/icons") {
        return false;
    }
    if src.ends_with(".gif") || src.contains("clear.gif") || src.contains("icon_") {
        return false;
    }
    true
}

/// True if the page is a login/captcha wall rather than real content --
/// same check httpFetcher.ts and worker.ts both use before trusting a response.
pub fn needs_auth(html: &Html) -> bool {
    let form_sel = sel("form");
    let login_box_sel = sel("#login-box");
    let has_captcha_form = html
        .select(&form_sel)
        .any(|f| f.text().collect::<String>().to_lowercase().contains("captcha"));
    has_captcha_form || html.select(&login_box_sel).next().is_some()
}

/// Same check as `needs_auth`, taking raw HTML text directly -- for callers
/// (lib.rs's `check_source_login`) that just have a fetched page body and
/// don't want to pull in `scraper::Html` themselves.
///
/// NOTE: only verified against rutracker's actual markup (`#login-box`, a
/// captcha-mentioning form). nnmclub.to is a different phpBB fork and its
/// logged-out page has NOT been confirmed to use the same `#login-box` id --
/// phpBB forks commonly rename or restructure this. This is a best-effort
/// reuse, not a confirmed-correct check for nnmclub; if login detection
/// reports the wrong thing there, this is the first place to look.
pub fn page_needs_auth(raw_html: &str) -> bool {
    needs_auth(&Html::parse_document(raw_html))
}

pub fn parse_post_details(raw_html: &str, url: &str) -> Option<ParsedPost> {
    let html = Html::parse_document(raw_html);

    if needs_auth(&html) {
        return None;
    }

    let title = text_of(&html, ".maintitle");
    let magnet = html
        .select(&sel("a.magnet-link"))
        .next()
        .and_then(|e| e.value().attr("href"))
        .unwrap_or_default()
        .to_string();

    if title.is_empty() || magnet.is_empty() {
        return None;
    }

    // --- image extraction, same multi-strategy order as httpFetcher.ts ---
    let mut post_image: Option<String> = None;

    // Strategy A: <var class="postImg">/<var class="postImgAligned"> title attr (lazy-load)
    for el in html.select(&sel("var.postImg, var.postImgAligned")) {
        if let Some(title_attr) = el.value().attr("title") {
            if is_valid_image(title_attr) {
                post_image = Some(title_attr.to_string());
                break;
            }
        }
    }

    // Strategy B: <img class="postImgAligned">/<img class="postImg"> src
    if post_image.is_none() {
        for el in html.select(&sel("img.postImgAligned, img.postImg")) {
            if let Some(src) = el.value().attr("src") {
                if is_valid_image(src) {
                    post_image = Some(src.to_string());
                    break;
                }
            }
        }
    }

    // Strategy C: BBCode [img]URL[/img] inside the raw textarea
    if post_image.is_none() {
        static BBCODE_RE: OnceLock<Regex> = OnceLock::new();
        let re = BBCODE_RE.get_or_init(|| {
            Regex::new(r"(?i)\[img(?:=[^\]]+)?\](https?://[^\[]+\.(?:png|jpg|jpeg|webp)[^\[]*?)\[/img\]").unwrap()
        });
        let textarea_text: String = html
            .select(&sel("textarea"))
            .next()
            .map(|e| e.text().collect())
            .unwrap_or_default();
        if let Some(m) = re.captures(&textarea_text) {
            let candidate = m.get(1).unwrap().as_str().trim();
            if is_valid_image(candidate) {
                post_image = Some(candidate.to_string());
            }
        }
    }

    // Strategy D: last resort -- first valid image in the post body
    if post_image.is_none() {
        for el in html.select(&sel(".post_body img")) {
            if let Some(src) = el.value().attr("src") {
                if is_valid_image(src) {
                    post_image = Some(src.to_string());
                    break;
                }
            }
        }
    }

    let mut description = text_of(&html, ".post_body");
    static SCREENSHOTS_RE: OnceLock<Regex> = OnceLock::new();
    static HELP_RE: OnceLock<Regex> = OnceLock::new();
    static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();
    // Rust's `regex` crate has no lookahead support, unlike the original TS regex
    // (`(?=\n\n|\n[A-Z]|[A-Z]|$)`). This consumes the trailing blank line instead of
    // just stopping before it -- cosmetically different, but the whitespace-collapse
    // pass right below already normalizes runs of blank lines, so it's a non-issue.
    let screenshots_re = SCREENSHOTS_RE.get_or_init(|| {
        Regex::new(r"(?is)Скриншоты.*?(?:\n\n|\z)").unwrap()
    });
    let help_re = HELP_RE.get_or_init(|| Regex::new(r"(?is)Помощь Рутрекеру.*$").unwrap());
    let whitespace_re = WHITESPACE_RE.get_or_init(|| Regex::new(r"\s{3,}").unwrap());
    description = screenshots_re.replace_all(&description, "").to_string();
    description = help_re.replace_all(&description, "").to_string();
    description = whitespace_re.replace_all(&description, "\n\n").to_string();
    description = description.trim().to_string();

    let text_body = format!("{} {}", title, description).to_lowercase();
    let tags = if text_body.contains("quest 3s") {
        "Quest 3S"
    } else if text_body.contains("quest 3") {
        "Quest 3"
    } else if text_body.contains("quest 2") {
        "Quest 2"
    } else if text_body.contains("quest") {
        "Quest"
    } else if text_body.contains("pcvr") {
        "PCVR"
    } else {
        "VR"
    };

    let size_from_elem = text_of(&html, "#tor-size-humn");
    static SIZE_RE: OnceLock<Regex> = OnceLock::new();
    let size_re = SIZE_RE.get_or_init(|| Regex::new(r"(?i)Размер:\s*([\d.]+\s*(GB|MB|KB))").unwrap());
    let size = if !size_from_elem.is_empty() {
        static NBSP_RE: OnceLock<Regex> = OnceLock::new();
        let nbsp_re = NBSP_RE.get_or_init(|| Regex::new(r"&nbsp;|\s+").unwrap());
        nbsp_re.replace_all(&size_from_elem, " ").to_string()
    } else if let Some(m) = size_re.captures(&description) {
        m.get(1).unwrap().as_str().to_string()
    } else {
        "Unknown".to_string()
    };

    let stats_text = text_of(&html, "#t-tor-stats");

    let seed_elem: i64 = text_of(&html, "span.seed b").parse().unwrap_or(0);
    static SEED_RE: OnceLock<Regex> = OnceLock::new();
    let seed_re = SEED_RE.get_or_init(|| Regex::new(r"(?i)Сиды:\s*(\d+)").unwrap());
    let seeds = if seed_elem != 0 {
        seed_elem
    } else {
        seed_re
            .captures(&stats_text)
            .and_then(|m| m.get(1).unwrap().as_str().parse().ok())
            .unwrap_or(0)
    };

    let leech_elem: i64 = text_of(&html, "span.leech b").parse().unwrap_or(0);
    static LEECH_RE: OnceLock<Regex> = OnceLock::new();
    let leech_re = LEECH_RE.get_or_init(|| Regex::new(r"(?i)Личи:\s*(\d+)").unwrap());
    let leeches = if leech_elem != 0 {
        leech_elem
    } else {
        leech_re
            .captures(&stats_text)
            .and_then(|m| m.get(1).unwrap().as_str().parse().ok())
            .unwrap_or(0)
    };

    static REG_RE: OnceLock<Regex> = OnceLock::new();
    let reg_re = REG_RE.get_or_init(|| {
        Regex::new(r"(?i)Зарегистрирован:\s*([\w\d\sа-яА-ЯёЁ\-.,]+)").unwrap()
    });
    let registered_at = reg_re
        .captures(&stats_text)
        .map(|m| m.get(1).unwrap().as_str().trim().to_string())
        .unwrap_or_default();

    static DL_RE: OnceLock<Regex> = OnceLock::new();
    let dl_re = DL_RE.get_or_init(|| {
        Regex::new(r"(?i)скачан:\s*(\d+)\s*раз|\.torrent скачан:\s*(\d+)").unwrap()
    });
    let torrent_downloads = dl_re
        .captures(&stats_text)
        .and_then(|m| m.get(1).or_else(|| m.get(2)))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);

    Some(ParsedPost {
        title,
        description,
        magnet,
        post_url: url.to_string(),
        tags: tags.to_string(),
        size,
        post_image,
        seeds,
        leeches,
        registered_at,
        torrent_downloads,
    })
}

/// Port of worker.ts's list-page row extraction (tracker.php / viewforum.php).
pub fn parse_list_page(raw_html: &str, is_tracker: bool) -> (Vec<ListItem>, bool, bool) {
    let html = Html::parse_document(raw_html);
    // Checking for a bare "login.php" substring was a false positive: rutracker's
    // normal logged-in header/footer also links to login.php (e.g. the logout
    // link), so that check fired even on a fully authenticated page. Rely on the
    // same signal the validated spike used: an actual login form or Cloudflare
    // challenge markup, via needs_auth().
    let auth_needed = needs_auth(&html);

    let mut items = Vec::new();

    if is_tracker {
        for row in html.select(&sel("#tor-tbl tr.tCenter, tr.tCenter")) {
            let link = row
                .select(&sel("a.tLink"))
                .next()
                .and_then(|e| e.value().attr("href"));
            let size = row
                .select(&sel("a.tr-dl"))
                .next()
                .map(|e| e.text().collect::<String>().replace('↓', "").trim().to_string())
                .unwrap_or_default();
            if let Some(link) = link {
                let full_url = if link.starts_with("http") {
                    link.to_string()
                } else {
                    format!("https://rutracker.org/forum/{}", link)
                };
                items.push(ListItem { link: full_url, size });
            }
        }
    } else {
        for el in html.select(&sel("a.torTopic")) {
            if let Some(link) = el.value().attr("href") {
                let full_url = if link.starts_with("http") {
                    link.to_string()
                } else {
                    format!("https://rutracker.org/forum/{}", link)
                };
                items.push(ListItem {
                    link: full_url,
                    size: "Unknown".to_string(),
                });
            }
        }
    }

    let has_next = html
        .select(&sel("a.pg, a.p-next"))
        .any(|e| e.text().collect::<String>().contains("След"));

    (items, has_next, auth_needed)
}

// ---- nnmclub.to (Task #15: additional torrent source) ----
// nnmclub.to runs a phpBB fork visually close to rutracker's, but its own
// class names -- confirmed against a real working scraper the user supplied
// (Python + BeautifulSoup, selecting table.forumline / tr.prow1,prow2 /
// a.genmed / download.php-or-magnet links / <u> size / span.seedmed or a
// bare-digit <b> for seeds). Ported 1:1 below rather than reusing rutracker's
// selectors, which are a different (if similar-looking) skin.
//
// Unlike rutracker's flow, nnmclub's own listing row already carries the
// title AND (when available) a direct magnet link -- no separate detail-page
// fetch is needed to get a usable torrent, so there's no nnmclub equivalent
// of `parse_post_details` yet. A `download.php`-only row (no magnet href)
// still needs that file resolved to an info-hash before it's usable; that
// resolution isn't implemented yet.

const NNMCLUB_BASE: &str = "https://nnmclub.to";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NnmclubItem {
    pub title: String,
    pub topic_url: String,
    /// A magnet: URI when the row exposes one directly; a download.php URL
    /// (not yet resolved to a magnet) otherwise; empty if neither was found.
    pub download_url: String,
    pub size: String,
    pub seeds: i64,
}

fn nnmclub_abs_url(href: &str) -> String {
    if href.starts_with("http") || href.starts_with("magnet:") {
        href.to_string()
    } else {
        format!("{NNMCLUB_BASE}/forum/{href}")
    }
}

/// Port of the user-supplied scraper's `parse_tracker_table()`.
pub fn parse_nnmclub_list_page(raw_html: &str) -> Vec<NnmclubItem> {
    let html = Html::parse_document(raw_html);
    let mut items = Vec::new();

    // NNM-Club's search-results page has TWO tables with class "forumline":
    // the first is the search/category-filter panel at the top of the page,
    // and only the SECOND -- the actual results table -- also carries the
    // "tablesorter" class. Selecting plain "table.forumline" and taking
    // .next() (the first match) was silently grabbing the filter panel every
    // time, which has no prow1/prow2 rows at all -- that's why this always
    // returned zero items even when the search genuinely had results.
    // Confirmed directly against the live site's HTML, not guessed.
    let Some(table) = html.select(&sel("table.forumline.tablesorter")).next() else {
        return items;
    };

    let row_sel = sel("tr.prow1, tr.prow2");
    let cell_sel = sel("td");
    let title_sel = sel("a.genmed");
    let dl_sel = sel(r#"a[href*="download.php"], a[href^="magnet:"]"#);
    let size_sel = sel("u");
    let seed_sel = sel("span.seedmed");
    let bold_sel = sel("b");

    for row in table.select(&row_sel) {
        if row.select(&cell_sel).count() < 5 {
            continue;
        }

        let Some(title_tag) = row.select(&title_sel).next() else {
            continue;
        };
        let title = title_tag.text().collect::<String>().trim().to_string();
        let href = title_tag.value().attr("href").unwrap_or_default();
        let topic_url = nnmclub_abs_url(href);

        let download_url = row
            .select(&dl_sel)
            .next()
            .and_then(|e| e.value().attr("href"))
            .map(nnmclub_abs_url)
            .unwrap_or_default();

        let size = row
            .select(&size_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown".to_string());

        // span.seedmed first; else the first <b> whose text is purely digits
        // (mirrors the Python fallback: `row.find("b", string=lambda s: s and
        // s.isdigit())`).
        let seeds = row
            .select(&seed_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .or_else(|| {
                row.select(&bold_sel)
                    .map(|e| e.text().collect::<String>().trim().to_string())
                    .find(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()))
            })
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        items.push(NnmclubItem { title, topic_url, download_url, size, seeds });
    }

    items
}
