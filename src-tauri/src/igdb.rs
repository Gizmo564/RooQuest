//! Best-effort IGDB metadata lookup, used to fill in description/genre/
//! developer/publisher/cover-art for a freshly-scraped game. Auth is via
//! Twitch's OAuth2 client-credentials flow (IGDB is owned by Twitch/IGDB
//! Inc. and uses Twitch's token endpoint), then a single Apicalypse search
//! query against IGDB's /games endpoint. Any failure here (missing
//! credentials, network error, no match) should never fail or block the
//! scrape that triggered it -- callers treat every error as "skip metadata
//! for this game" rather than propagating it.

use regex::Regex;
use serde::Deserialize;

/// What we pull out of an IGDB match and feed into db::apply_igdb_metadata.
/// Every field is optional because IGDB entries are inconsistently filled
/// in, and a missing field here just means that column doesn't get updated
/// (apply_igdb_metadata uses COALESCE so it never overwrites existing data
/// with a NULL).
#[derive(Debug, Default, Clone)]
pub struct IgdbMetadata {
    pub description: Option<String>,
    pub genre: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize, Default)]
struct IgdbGenre {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
struct IgdbCompany {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
struct IgdbInvolvedCompany {
    company: Option<IgdbCompany>,
    #[serde(default)]
    developer: bool,
    #[serde(default)]
    publisher: bool,
}

#[derive(Deserialize, Default)]
struct IgdbCover {
    image_id: Option<String>,
}

#[derive(Deserialize, Default)]
struct IgdbGame {
    #[allow(dead_code)]
    name: Option<String>,
    summary: Option<String>,
    #[serde(default)]
    genres: Vec<IgdbGenre>,
    #[serde(default)]
    involved_companies: Vec<IgdbInvolvedCompany>,
    cover: Option<IgdbCover>,
}

/// Exchanges a Twitch Client ID/Secret for a short-lived app access token.
/// This is the standard OAuth2 "client_credentials" grant -- no user login
/// involved, since IGDB access is an app-level credential, not a per-user
/// one.
pub async fn get_token(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
) -> Result<String, String> {
    let resp = http
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("twitch token request failed: {status} {body}"));
    }

    let parsed: TokenResponse = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parsed.access_token)
}

/// Scraped titles look like
/// "[VR Meta Quest 2/Quest Pro/Quest 3/Quest 3S] Zombomber! v0.5 [Экшен,
/// Приключенческие, Аркады, ENG]" on RuTracker-style sources (a leading
/// platform-list bracket, the actual game name, a version number, and a
/// trailing genre/language bracket) or like "The Backrooms: Mass Extinction
/// (2022) [En] (1.2.2 Beta) Repack FitGirl [VR]" on NNM-Club-style repack
/// sources (year/version PARENTHESES and an un-bracketed "Repack <group>"
/// phrase, not just brackets). IGDB's search only works well against the
/// bare name, so this strips all of that scaffolding down to just the game
/// name before searching, repeatedly peeling off one trailing chunk
/// (bracket, parenthetical, "Repack ...", version number, or a trailing
/// scene-style "-GROUP" / "-GROUP R3" release tag) at a time, in whatever
/// order they actually appear, until nothing more comes off. Confirmed
/// against real titles from both sources rather than assumed. Best-effort:
/// titles that don't follow either shape are left as close to untouched as
/// the individual patterns allow, rather than failing the lookup outright.
fn clean_title_for_search(title: &str) -> String {
    let mut cleaned = title.trim().to_string();

    // Leading "[...]" -- the platform list (e.g. "[VR Meta Quest 2/Quest
    // Pro/Quest 3/Quest 3S]").
    if let Ok(re) = Regex::new(r"^\[[^\]]*\]\s*") {
        cleaned = re.replace(&cleaned, "").into_owned();
    }

    let trailing_bracket = Regex::new(r"\s*\[[^\]]*\]\s*$").unwrap();
    let trailing_paren = Regex::new(r"\s*\([^)]*\)\s*$").unwrap();
    // "Repack <group>", "Scene <group>", "License <group/platform>" are
    // NNM-Club's three standard release-type markers (confirmed against
    // real scraped titles -- "Repack FitGirl", "Scene Rune", "License
    // GOG", etc.), each followed by free text through the end of the
    // title. Checked directly against the live library: titles still
    // carrying one of these when searched were markedly less likely to
    // get an IGDB match than ones where it had already been stripped,
    // especially for shorter/less distinctive game names where the extra
    // words throw off IGDB's fuzzy search.
    let trailing_release_marker = Regex::new(r"(?i)\s+(repack|scene|license)\b.*$").unwrap();
    let trailing_version = Regex::new(r"(?i)\s+v[0-9][0-9a-z.+\-]*\s*$").unwrap();
    let trailing_scene_group = Regex::new(r"(?i)\s+-[a-z0-9]{2,10}(\s+r[0-9]+)?\s*$").unwrap();

    loop {
        let before = cleaned.clone();
        cleaned = trailing_bracket.replace(&cleaned, "").into_owned();
        cleaned = trailing_paren.replace(&cleaned, "").into_owned();
        cleaned = trailing_release_marker.replace(&cleaned, "").into_owned();
        cleaned = trailing_version.replace(&cleaned, "").into_owned();
        cleaned = trailing_scene_group.replace(&cleaned, "").into_owned();
        cleaned = cleaned.trim().to_string();
        if cleaned == before {
            break;
        }
    }

    if cleaned.is_empty() {
        title.trim().to_string()
    } else {
        cleaned
    }
}

/// Looks up `title` on IGDB and returns the best-guess metadata for the
/// single closest match (IGDB's own search ranking, `limit 1`), or `Ok(None)`
/// if nothing came back. Returns `Err` only for actual request/transport
/// failures -- callers should still treat that as "couldn't get metadata
/// this time" rather than a hard failure.
pub async fn fetch_metadata(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    title: &str,
) -> Result<Option<IgdbMetadata>, String> {
    if client_id.trim().is_empty() || client_secret.trim().is_empty() {
        return Ok(None);
    }

    let token = get_token(http, client_id, client_secret).await?;
    fetch_metadata_with_token(http, &token, client_id, title).await
}

/// Same lookup as `fetch_metadata`, but takes an already-fetched Twitch
/// access token instead of exchanging credentials for a new one every call.
/// Used by the bulk "re-scrape IGDB for everything" backfill so a run over
/// hundreds of games does a single token exchange instead of one per game.
pub async fn fetch_metadata_with_token(
    http: &reqwest::Client,
    token: &str,
    client_id: &str,
    title: &str,
) -> Result<Option<IgdbMetadata>, String> {
    // Apicalypse query language, not JSON -- this is IGDB's own query
    // syntax. Escape embedded double quotes in the title so a game like
    // `Among Us` style title containing a `"` doesn't break the query.
    let cleaned_title = clean_title_for_search(title);
    let safe_title = cleaned_title.replace('"', "'");
    let query = format!(
        "search \"{safe_title}\"; fields name,summary,genres.name,involved_companies.company.name,involved_companies.developer,involved_companies.publisher,cover.image_id; limit 1;"
    );

    let resp = http
        .post("https://api.igdb.com/v4/games")
        .header("Client-ID", client_id)
        .header("Authorization", format!("Bearer {token}"))
        .body(query)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("igdb games request failed: {status} {body}"));
    }

    let games: Vec<IgdbGame> = resp.json().await.map_err(|e| e.to_string())?;
    let Some(game) = games.into_iter().next() else {
        return Ok(None);
    };

    let genre = game
        .genres
        .iter()
        .filter_map(|g| g.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let genre = if genre.is_empty() { None } else { Some(genre) };

    let developer = game
        .involved_companies
        .iter()
        .find(|c| c.developer)
        .and_then(|c| c.company.as_ref())
        .and_then(|c| c.name.clone());

    let publisher = game
        .involved_companies
        .iter()
        .find(|c| c.publisher)
        .and_then(|c| c.company.as_ref())
        .and_then(|c| c.name.clone());

    let image_url = game
        .cover
        .and_then(|c| c.image_id)
        .map(|id| format!("https://images.igdb.com/igdb/image/upload/t_cover_big/{id}.jpg"));

    Ok(Some(IgdbMetadata {
        description: game.summary,
        genre,
        developer,
        publisher,
        image_url,
    }))
}
