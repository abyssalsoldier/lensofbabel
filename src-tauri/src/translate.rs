//! Client for Google Translate's free, unofficial `translate_a/single` endpoint.
//!
//! This endpoint has no SLA and is rate-limited, so all HTTP details stay in this
//! file, which makes it easy to swap in another backend later.

use serde::Serialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const ENDPOINT: &str = "https://translate.googleapis.com/translate_a/single";
/// Google rejects very long requests; stay well under the limit.
const MAX_CHUNK: usize = 4500;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Translation {
    pub text: String,
    pub detected_source: Option<String>,
    pub target: String,
}

/// Values for the endpoint's `client` parameter, tried in order.
const CLIENT_IDS: [&str; 2] = ["gtx", "dict-chrome-ex"];

struct Blocked;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    client_idx: Arc<AtomicUsize>,
}

impl Client {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            // Honest, identifiable User-Agent.
            .user_agent(concat!("LensOfBabel/", env!("CARGO_PKG_VERSION"), " (Windows desktop app)"))
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build HTTP client");
        Self { http, client_idx: Arc::new(AtomicUsize::new(0)) }
    }

    pub async fn translate(&self, text: &str, source: &str, target: &str) -> Result<Translation, String> {
        let source = if source.is_empty() { "auto" } else { source };
        if text.trim().is_empty() {
            return Ok(Translation { text: String::new(), detected_source: None, target: target.to_string() });
        }
        let mut out = String::with_capacity(text.len());
        let mut detected = None;
        for chunk in split_chunks(text, MAX_CHUNK) {
            let (t, d) = self.translate_chunk(chunk, source, target).await?;
            out.push_str(&t);
            if detected.is_none() {
                detected = d;
            }
        }
        Ok(Translation { text: out, detected_source: detected, target: target.to_string() })
    }

    async fn translate_chunk(&self, chunk: &str, source: &str, target: &str) -> Result<(String, Option<String>), String> {
        // Google sometimes blocks one `client` value for certain HTTP clients with
        // its "automated queries" page (HTTP 429). Try each known value once and
        // remember the one that works.
        let first = self.client_idx.load(Ordering::Relaxed);
        let mut last_err = String::new();
        for i in 0..CLIENT_IDS.len() {
            let idx = (first + i) % CLIENT_IDS.len();
            match self.request(CLIENT_IDS[idx], chunk, source, target).await {
                Err(Blocked) => {
                    last_err = "Google Translate is rate-limiting this computer. Wait a minute and try again.".into();
                }
                Ok(result) => {
                    self.client_idx.store(idx, Ordering::Relaxed);
                    return result;
                }
            }
        }
        Err(last_err)
    }

    async fn request(
        &self,
        client_id: &str,
        chunk: &str,
        source: &str,
        target: &str,
    ) -> Result<Result<(String, Option<String>), String>, Blocked> {
        let sent = self
            .http
            .post(ENDPOINT)
            .query(&[("client", client_id), ("sl", source), ("tl", target), ("dt", "t")])
            .form(&[("q", chunk)])
            .send()
            .await;
        let resp = match sent {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Ok(Err("The translation service timed out.".into())),
            Err(e) => return Ok(Err(format!("Network error: {e}"))),
        };
        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(Blocked);
        }
        Ok(Self::read(resp).await)
    }

    async fn read(resp: reqwest::Response) -> Result<(String, Option<String>), String> {
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("The translation service returned HTTP {status}."));
        }
        let body = resp.text().await.map_err(|e| format!("Could not read the response: {e}"))?;
        parse_response(&body)
    }
}

/// Parses `[[["translated","original",...], ...], null, "detected", ...]`.
pub fn parse_response(body: &str) -> Result<(String, Option<String>), String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|_| "Unexpected response from the translation service.".to_string())?;
    let segments = v
        .get(0)
        .and_then(|s| s.as_array())
        .ok_or_else(|| "Unexpected response from the translation service.".to_string())?;
    let mut out = String::new();
    for seg in segments {
        if let Some(s) = seg.get(0).and_then(|x| x.as_str()) {
            out.push_str(s);
        }
    }
    let detected = v.get(2).and_then(|d| d.as_str()).map(str::to_string);
    Ok((out, detected))
}

/// Splits `text` into contiguous slices of at most `max` bytes. It prefers to
/// break after a newline, then after sentence punctuation, then after a space.
pub fn split_chunks(text: &str, max: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while rest.len() > max {
        let mut limit = max;
        while !rest.is_char_boundary(limit) {
            limit -= 1;
        }
        let window = &rest[..limit];
        let cut = window
            .rfind('\n')
            .map(|i| i + 1)
            .or_else(|| {
                [". ", "! ", "? ", "。", "！", "？"]
                    .iter()
                    .filter_map(|p| window.rfind(p).map(|i| i + p.len()))
                    .max()
            })
            .or_else(|| window.rfind(' ').map(|i| i + 1))
            .filter(|&i| i > 0)
            .unwrap_or(limit);
        chunks.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        chunks.push(rest);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_segments_and_detected_language() {
        let body = r#"[[["Hola. ","Hello. ",null,null,10],["¿Cómo estás?","How are you?",null,null,10]],null,"en",null,null,null,1,[],[["en"],null,[1],["en"]]]"#;
        let (text, detected) = parse_response(body).unwrap();
        assert_eq!(text, "Hola. ¿Cómo estás?");
        assert_eq!(detected.as_deref(), Some("en"));
    }

    #[test]
    fn parses_without_detected_language() {
        let (text, detected) = parse_response(r#"[[["Bonjour","Hello",null,null,1]]]"#).unwrap();
        assert_eq!(text, "Bonjour");
        assert_eq!(detected, None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_response("<html>blocked</html>").is_err());
        assert!(parse_response("{}").is_err());
    }

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(split_chunks("hello world", 100), vec!["hello world"]);
    }

    #[test]
    fn chunks_prefer_newlines_and_rejoin_exactly() {
        let text = "line one\nline two\nline three";
        let chunks = split_chunks(text, 12);
        assert!(chunks.iter().all(|c| c.len() <= 12));
        assert_eq!(chunks[0], "line one\n");
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn chunks_respect_utf8_boundaries() {
        let text = "日本語のテキスト".repeat(50);
        let chunks = split_chunks(&text, 100);
        assert!(chunks.iter().all(|c| c.len() <= 100));
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    #[ignore = "hits the live Google endpoint"]
    fn live_translation() {
        let client = Client::new();
        let t = tauri::async_runtime::block_on(client.translate("Hello, world", "auto", "es")).unwrap();
        assert!(!t.text.is_empty());
        assert_eq!(t.detected_source.as_deref(), Some("en"));
        println!("{t:?}");
    }
}
