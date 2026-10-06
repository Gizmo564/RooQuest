//! Minimal, self-contained bencode reader -- just enough to turn a raw
//! `.torrent` file's bytes into a magnet URI. This exists because
//! NNM-Club's search results sometimes expose only a `download.php` link
//! (the .torrent file itself, behind the authenticated webview session)
//! rather than a ready-made `magnet:` URI -- RuTracker's rows always have
//! one directly, NNM-Club's don't always.
//!
//! A magnet link only needs the info-hash (SHA-1 of the raw bytes of the
//! torrent's `info` dict, exactly as encoded -- NOT a re-serialization of a
//! parsed value, since that could legally differ byte-for-byte and would
//! produce the wrong hash) plus, optionally, a display name and tracker
//! URLs to speed up peer discovery. This module only ever reads bencode,
//! never writes it, which keeps it small and easy to get right against the
//! (simple, stable) bencode spec rather than against a moving library API.

use sha1::{Digest, Sha1};

#[derive(Debug, Clone)]
enum BValue {
    Str(Vec<u8>),
    List(Vec<BValue>),
    #[allow(dead_code)]
    Int(i64),
    #[allow(dead_code)]
    Dict(Vec<(Vec<u8>, BValue)>),
}

/// Advances past one bencode value starting at `data[pos]`, returning the
/// index just past it. Handles all four bencode types: integers (`i123e`),
/// byte strings (`5:hello`), lists (`l...e`), and dicts (`d...e`).
fn skip_value(data: &[u8], pos: usize) -> Option<usize> {
    match *data.get(pos)? {
        b'i' => {
            let end = find_byte(data, pos + 1, b'e')?;
            Some(end + 1)
        }
        b'l' => {
            let mut p = pos + 1;
            while data.get(p)? != &b'e' {
                p = skip_value(data, p)?;
            }
            Some(p + 1)
        }
        b'd' => {
            let mut p = pos + 1;
            while data.get(p)? != &b'e' {
                p = skip_value(data, p)?; // key (always a string, but skip_value handles any type)
                p = skip_value(data, p)?; // value
            }
            Some(p + 1)
        }
        b'0'..=b'9' => {
            let colon = find_byte(data, pos, b':')?;
            let len: usize = std::str::from_utf8(&data[pos..colon]).ok()?.parse().ok()?;
            let start = colon + 1;
            Some(start.checked_add(len)?)
        }
        _ => None,
    }
}

/// Decodes one bencode value into an owned `BValue`, for the handful of
/// top-level keys (`name`, `announce`, `announce-list`) we actually want
/// the *parsed* contents of, as opposed to `info`, which we only ever want
/// the raw byte span of (see `info_hash_hex`).
fn decode_value(data: &[u8], pos: usize) -> Option<(BValue, usize)> {
    match *data.get(pos)? {
        b'i' => {
            let end = find_byte(data, pos + 1, b'e')?;
            let n: i64 = std::str::from_utf8(&data[pos + 1..end]).ok()?.parse().ok()?;
            Some((BValue::Int(n), end + 1))
        }
        b'l' => {
            let mut p = pos + 1;
            let mut items = Vec::new();
            while data.get(p)? != &b'e' {
                let (v, next) = decode_value(data, p)?;
                items.push(v);
                p = next;
            }
            Some((BValue::List(items), p + 1))
        }
        b'd' => {
            let mut p = pos + 1;
            let mut pairs = Vec::new();
            while data.get(p)? != &b'e' {
                let (k, next) = decode_value(data, p)?;
                let key = match k {
                    BValue::Str(s) => s,
                    _ => return None, // dict keys are always bencode strings
                };
                let (v, next2) = decode_value(data, next)?;
                pairs.push((key, v));
                p = next2;
            }
            Some((BValue::Dict(pairs), p + 1))
        }
        b'0'..=b'9' => {
            let colon = find_byte(data, pos, b':')?;
            let len: usize = std::str::from_utf8(&data[pos..colon]).ok()?.parse().ok()?;
            let start = colon + 1;
            let end = start.checked_add(len)?;
            Some((BValue::Str(data.get(start..end)?.to_vec()), end))
        }
        _ => None,
    }
}

fn find_byte(data: &[u8], from: usize, target: u8) -> Option<usize> {
    data[from..].iter().position(|&b| b == target).map(|i| i + from)
}

fn dict_get<'a>(pairs: &'a [(Vec<u8>, BValue)], key: &str) -> Option<&'a BValue> {
    pairs.iter().find(|(k, _)| k == key.as_bytes()).map(|(_, v)| v)
}

/// Walks the top-level dict just far enough to find the raw byte span of
/// the `info` value, then SHA-1-hashes those exact bytes -- this is the
/// torrent's info-hash (`btih`), the only thing a magnet link strictly
/// needs. Returns `None` for anything that isn't a well-formed .torrent
/// file (doesn't start with a dict, or has no `info` key).
fn info_hash_hex(data: &[u8]) -> Option<String> {
    if data.first()? != &b'd' {
        return None;
    }
    let mut p = 1usize;
    while data.get(p)? != &b'e' {
        let key_start = p;
        let key_end = skip_value(data, p)?;
        let value_start = key_end;
        let value_end = skip_value(data, value_start)?;
        // Keys are always bencode strings; decode just this one to compare.
        if let Some((BValue::Str(key_bytes), _)) = decode_value(data, key_start) {
            if key_bytes == b"info" {
                let mut hasher = Sha1::new();
                hasher.update(&data[value_start..value_end]);
                let digest = hasher.finalize();
                return Some(digest.iter().map(|b| format!("{b:02x}")).collect());
            }
        }
        p = value_end;
    }
    None
}

/// Best-effort tracker URLs from the top-level `announce` / `announce-list`
/// keys, for `&tr=` params -- purely an optimization (DHT + PEX still work
/// without them, since librqbit's Session runs its own DHT by default), so
/// any parse failure here just means an empty list rather than an error.
fn trackers(data: &[u8]) -> Vec<String> {
    let Some((BValue::Dict(pairs), _)) = decode_value(data, 0) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(BValue::Str(s)) = dict_get(&pairs, "announce") {
        if let Ok(s) = String::from_utf8(s.clone()) {
            out.push(s);
        }
    }
    if let Some(BValue::List(tiers)) = dict_get(&pairs, "announce-list") {
        for tier in tiers {
            if let BValue::List(urls) = tier {
                for u in urls {
                    if let BValue::Str(s) = u {
                        if let Ok(s) = String::from_utf8(s.clone()) {
                            if !out.contains(&s) {
                                out.push(s);
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Turns a raw `.torrent` file's bytes into a `magnet:` URI, using
/// `display_name` (the title we already scraped from the page -- no need
/// to also pull `info.name` back out of the file) for `dn=`. Returns
/// `None` if the bytes don't parse as a well-formed torrent file at all.
pub fn to_magnet(data: &[u8], display_name: &str) -> Option<String> {
    let hash = info_hash_hex(data)?;
    let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
    if !display_name.is_empty() {
        magnet.push_str("&dn=");
        magnet.push_str(&urlencoding::encode(display_name));
    }
    for tr in trackers(data) {
        magnet.push_str("&tr=");
        magnet.push_str(&urlencoding::encode(&tr));
    }
    Some(magnet)
}
