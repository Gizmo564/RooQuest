//! Port of backend/src/utils/translate.ts -- same free Google Translate endpoint,
//! same chunking strategy for long text.

const MAX_CHUNK_SIZE: usize = 3500;

pub async fn translate_text(client: &reqwest::Client, text: &str, target_lang: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    if text.chars().count() <= MAX_CHUNK_SIZE {
        return translate_single_chunk(client, text, target_lang)
            .await
            .unwrap_or_else(|| text.to_string());
    }

    let chunks = split_into_chunks(text, MAX_CHUNK_SIZE);
    let mut translated = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        translated.push(
            translate_single_chunk(client, &chunk, target_lang)
                .await
                .unwrap_or(chunk),
        );
    }
    translated.join("\n")
}

async fn translate_single_chunk(client: &reqwest::Client, text: &str, target_lang: &str) -> Option<String> {
    // The free translate_a/single endpoint will intermittently respond with
    // an HTML "Sorry... automated queries" block page (HTTP 429) instead of
    // JSON once it decides an IP is sending too many requests too fast --
    // confirmed by hand against the live endpoint, and confirmed to have
    // actually triggered during a prior ~1700-request backfill run. There's
    // no paid API key configured, so the only real lever here is staying
    // well under whatever threshold tripped that: a few-second pacing delay
    // before every request (not just after a failure), plus a much longer
    // backoff before the one retry. This makes a full-library pass slow
    // (seconds per chunk, not milliseconds) but it's the only way to avoid
    // re-triggering the same block on a big run.
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    if let Some(result) = try_translate_once(client, text, target_lang).await {
        return Some(result);
    }

    tokio::time::sleep(std::time::Duration::from_millis(8000)).await;
    try_translate_once(client, text, target_lang).await
}

async fn try_translate_once(client: &reqwest::Client, text: &str, target_lang: &str) -> Option<String> {
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={}&dt=t&q={}",
        target_lang,
        urlencoding::encode(text)
    );

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;

    // Response shape: [[[translatedChunk, originalChunk, ...], ...], ...]
    let segments = body.get(0)?.as_array()?;
    let mut out = String::new();
    for seg in segments {
        if let Some(piece) = seg.get(0).and_then(|v| v.as_str()) {
            out.push_str(piece);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn split_into_chunks(text: &str, max_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.chars().count() <= max_size {
            chunks.push(remaining.to_string());
            break;
        }

        let boundary = find_split_point(remaining, max_size, '\n')
            .or_else(|| find_split_point(remaining, max_size, '.'))
            .unwrap_or(byte_index_at_char(remaining, max_size));

        let (head, tail) = remaining.split_at(boundary);
        chunks.push(head.to_string());
        remaining = tail.trim_start();
    }

    chunks
}

/// Finds the byte index of the last occurrence of `marker` at or before
/// char-position `max_size`, provided it isn't absurdly early (< half of max_size).
fn find_split_point(text: &str, max_size: usize, marker: char) -> Option<usize> {
    let limit_byte = byte_index_at_char(text, max_size);
    let prefix = &text[..limit_byte];
    let idx = prefix.rfind(marker)?;
    let char_pos = text[..idx].chars().count();
    if char_pos < max_size / 2 {
        None
    } else {
        Some(idx + marker.len_utf8())
    }
}

fn byte_index_at_char(text: &str, char_count: usize) -> usize {
    text.char_indices()
        .nth(char_count)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}
