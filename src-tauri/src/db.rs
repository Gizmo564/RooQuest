//! Port of backend/src/db/sqlite.ts -- same schema, same migration-by-ALTER-TABLE
//! approach, same `~/Documents/<app folder>/database.sqlite` location (so this
//! can read the exact same DB file the Node app already created on a user's
//! machine; see db_path() for the VRSideForge -> RooQuest migration).

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Mutex;

pub struct DbState(pub Mutex<Connection>);

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Game {
    pub id: Option<i64>,
    pub title: Option<String>,
    pub translated_title: Option<String>,
    pub description: Option<String>,
    pub translated_description: Option<String>,
    pub magnet: Option<String>,
    pub post_url: Option<String>,
    pub tags: Option<String>,
    pub size: Option<String>,
    pub image_url: Option<String>,
    pub seeds: i64,
    pub leeches: i64,
    pub registered_at: Option<String>,
    pub torrent_downloads: i64,
    pub wishlist: i64,
    pub genre: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub languages: Option<String>,
    pub play_modes: Option<String>,
    pub igdb_matched: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct FailedGame {
    pub id: Option<i64>,
    pub post_url: Option<String>,
    pub size: Option<String>,
    pub error_message: Option<String>,
    pub attempts: i64,
}

/// Resolves to the same path the Node app used, including the (now
/// two-hop) migration through the app's earlier names: VRRookieDownloader
/// -> VRSideForge -> RooQuest. Whichever of those folders actually exists
/// gets renamed forward to the current one, so a user's existing
/// database.sqlite -- their whole library -- is found right where they
/// left it instead of the app starting over with an empty one.
pub fn db_path() -> std::path::PathBuf {
    let home = dirs::home_dir().expect("no home directory");
    let docs = home.join("Documents");
    let legacy_dir = docs.join("VRRookieDownloader");
    let old_dir = docs.join("VRSideForge");
    let new_dir = docs.join("RooQuest");

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
    new_dir.join("database.sqlite")
}

pub fn init_db() -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path())?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS games (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT,
            description TEXT,
            translated_description TEXT,
            magnet TEXT,
            post_url TEXT UNIQUE,
            tags TEXT,
            size TEXT,
            image_url TEXT,
            seeds INTEGER DEFAULT 0,
            leeches INTEGER DEFAULT 0,
            registered_at TEXT,
            torrent_downloads INTEGER DEFAULT 0,
            wishlist INTEGER DEFAULT 0,
            translated_title TEXT,
            genre TEXT,
            developer TEXT,
            publisher TEXT,
            version TEXT,
            languages TEXT,
            play_modes TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS session (
            id INTEGER PRIMARY KEY,
            cookies TEXT,
            user_agent TEXT
        );

        CREATE TABLE IF NOT EXISTS failed_games (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            post_url TEXT UNIQUE,
            size TEXT,
            error_message TEXT,
            attempts INTEGER DEFAULT 0,
            last_attempt DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        ",
    )?;

    // Same "ALTER TABLE, ignore if it already exists" migration style as sqlite.ts,
    // for anyone whose DB predates the full schema above.
    let migrations = [
        "ALTER TABLE games ADD COLUMN seeds INTEGER DEFAULT 0",
        "ALTER TABLE games ADD COLUMN leeches INTEGER DEFAULT 0",
        "ALTER TABLE games ADD COLUMN registered_at TEXT",
        "ALTER TABLE games ADD COLUMN torrent_downloads INTEGER DEFAULT 0",
        "ALTER TABLE games ADD COLUMN wishlist INTEGER DEFAULT 0",
        "ALTER TABLE games ADD COLUMN translated_title TEXT",
        "ALTER TABLE games ADD COLUMN genre TEXT",
        "ALTER TABLE games ADD COLUMN developer TEXT",
        "ALTER TABLE games ADD COLUMN publisher TEXT",
        "ALTER TABLE games ADD COLUMN version TEXT",
        "ALTER TABLE games ADD COLUMN languages TEXT",
        "ALTER TABLE games ADD COLUMN play_modes TEXT",
        "ALTER TABLE session ADD COLUMN user_agent TEXT",
        "ALTER TABLE games ADD COLUMN igdb_matched INTEGER DEFAULT 0",
    ];
    for m in migrations {
        let _ = conn.execute(m, []);
    }

    Ok(conn)
}

pub fn game_exists(conn: &Connection, post_url: &str) -> rusqlite::Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM games WHERE post_url = ?1",
        [post_url],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn insert_game(conn: &Connection, g: &Game) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO games (
            title, translated_title, description, translated_description, magnet, post_url,
            tags, size, image_url, seeds, leeches, registered_at, torrent_downloads,
            genre, developer, publisher, version, languages, play_modes
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        rusqlite::params![
            g.title, g.translated_title, g.description, g.translated_description, g.magnet,
            g.post_url, g.tags, g.size, g.image_url, g.seeds, g.leeches, g.registered_at,
            g.torrent_downloads, g.genre, g.developer, g.publisher, g.version, g.languages,
            g.play_modes
        ],
    )?;
    conn.execute(
        "DELETE FROM failed_games WHERE post_url = ?1",
        [&g.post_url],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn record_failed_game(conn: &Connection, post_url: &str, size: &str, error: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO failed_games (post_url, size, error_message, attempts) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(post_url) DO UPDATE SET attempts = attempts + 1, last_attempt = CURRENT_TIMESTAMP, error_message = ?3",
        rusqlite::params![post_url, size, error],
    )?;
    Ok(())
}

pub fn get_all_games(conn: &Connection) -> rusqlite::Result<Vec<Game>> {
    let mut stmt = conn.prepare("SELECT id, title, translated_title, description, translated_description, magnet, post_url, tags, size, image_url, seeds, leeches, registered_at, torrent_downloads, wishlist, genre, developer, publisher, version, languages, play_modes, igdb_matched FROM games ORDER BY id DESC")?;
    let rows = stmt.query_map([], |row| {
        Ok(Game {
            id: row.get(0)?,
            title: row.get(1)?,
            translated_title: row.get(2)?,
            description: row.get(3)?,
            translated_description: row.get(4)?,
            magnet: row.get(5)?,
            post_url: row.get(6)?,
            tags: row.get(7)?,
            size: row.get(8)?,
            image_url: row.get(9)?,
            seeds: row.get(10)?,
            leeches: row.get(11)?,
            registered_at: row.get(12)?,
            torrent_downloads: row.get(13)?,
            wishlist: row.get(14)?,
            genre: row.get(15)?,
            developer: row.get(16)?,
            publisher: row.get(17)?,
            version: row.get(18)?,
            languages: row.get(19)?,
            play_modes: row.get(20)?,
            igdb_matched: row.get(21).unwrap_or(0),
        })
    })?;
    rows.collect()
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct StoredSession {
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

pub fn get_session(conn: &Connection) -> rusqlite::Result<Option<StoredSession>> {
    let result = conn.query_row(
        "SELECT cookies, user_agent FROM session WHERE id = 1",
        [],
        |row| {
            Ok(StoredSession {
                cookies: row.get(0)?,
                user_agent: row.get(1)?,
            })
        },
    );
    match result {
        Ok(s) => Ok(Some(s)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn save_session(conn: &Connection, cookies: &str, user_agent: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO session (id, cookies, user_agent) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET cookies = ?1, user_agent = ?2",
        rusqlite::params![cookies, user_agent],
    )?;
    Ok(())
}

/// Port of `GET /api/games/:id` (the base row lookup only -- the inventory
/// cross-reference against a download path hasn't been ported yet, since the
/// inventory/.index.json subsystem itself isn't ported. Returns the plain row.
pub fn get_game_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<Game>> {
    let result = conn.query_row(
        "SELECT id, title, translated_title, description, translated_description, magnet, post_url, tags, size, image_url, seeds, leeches, registered_at, torrent_downloads, wishlist, genre, developer, publisher, version, languages, play_modes, igdb_matched FROM games WHERE id = ?1",
        [id],
        |row| {
            Ok(Game {
                id: row.get(0)?,
                title: row.get(1)?,
                translated_title: row.get(2)?,
                description: row.get(3)?,
                translated_description: row.get(4)?,
                magnet: row.get(5)?,
                post_url: row.get(6)?,
                tags: row.get(7)?,
                size: row.get(8)?,
                image_url: row.get(9)?,
                seeds: row.get(10)?,
                leeches: row.get(11)?,
                registered_at: row.get(12)?,
                torrent_downloads: row.get(13)?,
                wishlist: row.get(14)?,
                genre: row.get(15)?,
                developer: row.get(16)?,
                publisher: row.get(17)?,
                version: row.get(18)?,
                languages: row.get(19)?,
                play_modes: row.get(20)?,
                igdb_matched: row.get(21).unwrap_or(0),
            })
        },
    );
    match result {
        Ok(g) => Ok(Some(g)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Port of `PUT /api/games/:id` -- same fields, same full-overwrite (not
/// partial-patch) behavior as the original route.
pub fn update_game_fields(
    conn: &Connection,
    id: i64,
    title: Option<&str>,
    tags: Option<&str>,
    genre: Option<&str>,
    developer: Option<&str>,
    publisher: Option<&str>,
    version: Option<&str>,
    languages: Option<&str>,
    play_modes: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE games SET title = ?1, tags = ?2, genre = ?3, developer = ?4, publisher = ?5, version = ?6, languages = ?7, play_modes = ?8 WHERE id = ?9",
        rusqlite::params![title, tags, genre, developer, publisher, version, languages, play_modes, id],
    )?;
    Ok(())
}

/// Writes a freshly-translated title/description onto a game row. Used by
/// both the per-scrape translation step and the manual "retranslate" backfill
/// -- a plain overwrite (not COALESCE), since translation always has
/// something to write (translate_text falls back to the original text on
/// failure, so these are never NULL).
pub fn apply_translation(
    conn: &Connection,
    id: i64,
    translated_title: &str,
    translated_description: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE games SET translated_title = ?1, translated_description = ?2 WHERE id = ?3",
        rusqlite::params![translated_title, translated_description, id],
    )?;
    Ok(())
}

/// Applies IGDB-sourced metadata to a game row. Every field is COALESCE(new,
/// existing): when IGDB returns a value it wins (so a real cover does
/// replace the generic Steam-search thumbnail images.rs sets at scrape
/// time -- COALESCE(new_image, old_image) already prefers new_image
/// whenever it's Some), and when IGDB didn't return something the existing
/// value (usually parsed straight from the RuTracker post) is left alone
/// rather than clobbered with NULL.
///
/// Also marks the row `igdb_matched = 1`. This is the actual fix for the
/// manual backfill skipping almost the entire library: it used to decide
/// "already complete" by checking whether description/genre/developer/
/// publisher/image_url were merely non-NULL, but RuTracker's own post
/// parsing (and images.rs's Steam-search fallback) already fills all five
/// of those for nearly every game -- so it looked "complete" before IGDB
/// had ever been consulted, and `apply_igdb_metadata` (and thus the real
/// cover-art overwrite above) never actually ran. `igdb_matched` is the
/// real signal for "IGDB has already been applied to this row".
pub fn apply_igdb_metadata(
    conn: &Connection,
    id: i64,
    description: Option<&str>,
    genre: Option<&str>,
    developer: Option<&str>,
    publisher: Option<&str>,
    image_url: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE games SET
            description = COALESCE(?1, description),
            genre = COALESCE(?2, genre),
            developer = COALESCE(?3, developer),
            publisher = COALESCE(?4, publisher),
            image_url = COALESCE(?5, image_url),
            igdb_matched = 1
         WHERE id = ?6",
        rusqlite::params![description, genre, developer, publisher, image_url, id],
    )?;
    Ok(())
}

/// Port of `DELETE /api/games/:id`.
pub fn delete_game(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM games WHERE id = ?1", [id])?;
    Ok(())
}

/// Port of `POST /api/games/:id/wishlist`.
pub fn set_wishlist(conn: &Connection, id: i64, wishlist: bool) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE games SET wishlist = ?1 WHERE id = ?2",
        rusqlite::params![if wishlist { 1 } else { 0 }, id],
    )?;
    Ok(())
}

/// Port of the `SELECT * FROM games WHERE title LIKE ? OR title LIKE ? LIMIT 1`
/// query backend/src/index.ts's `/api/filesystem/scan-local-downloads` route
/// uses to guess which game a downloaded folder/APK belongs to. Returns just
/// (id, title) -- that's all the filesystem-scan logic needs.
pub fn find_game_by_title_like(
    conn: &Connection,
    pattern_a: &str,
    pattern_b: &str,
) -> rusqlite::Result<Option<(i64, String)>> {
    conn.query_row(
        "SELECT id, title FROM games WHERE title LIKE ?1 OR title LIKE ?2 LIMIT 1",
        rusqlite::params![pattern_a, pattern_b],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}
