//! Icons for the websites people dictate into, shown on the pill and in History: the site's favicon, or the icon its
//! home page links to. Kept in memory; a site with no icon is asked again after a minute.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::OnceCell;

type Pending = Arc<OnceCell<Option<String>>>;
static KNOWN: Mutex<Option<HashMap<String, (Option<String>, Instant)>>> = Mutex::new(None);
static PENDING: Mutex<Option<HashMap<String, Pending>>> = Mutex::new(None);

fn valid(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        && labels.last().is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()))
}

enum Got {
    Icon(String),
    Page { html: String, base: String },
}

async fn fetch(url: &str) -> Option<Got> {
    let r = crate::CLIENT.get(url).timeout(Duration::from_secs(5)).send().await.ok()?;
    if !r.status().is_success() {
        return None;
    }
    let base = r.url().to_string();
    let kind = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim().to_string();
    let body = r.bytes().await.ok()?;
    if body.is_empty() || body.len() > 300_000 {
        return None;
    }
    if kind.starts_with("text/html") {
        return Some(Got::Page { html: String::from_utf8_lossy(&body).to_string(), base });
    }
    let kind = if kind.starts_with("image/") { kind } else { "image/x-icon".into() };
    Some(Got::Icon(format!("data:{kind};base64,{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &body))))
}

async fn load(host: &str) -> Option<String> {
    if let Some(Got::Icon(i)) = fetch(&format!("https://{host}/favicon.ico")).await {
        return Some(i);
    }
    let Some(Got::Page { html, base }) = fetch(&format!("https://{host}/")).await else { return None };
    let link = crate::re!(r#"(?i)<link[^>]+rel=["'][^"']*icon[^"']*["'][^>]*>"#).find(&html).ok().flatten()?.as_str().to_string();
    let href = crate::re!(r#"(?i)href=["']([^"']+)["']"#).captures(&link).ok().flatten()?.get(1)?.as_str().replace("&amp;", "&");
    let url = reqwest::Url::parse(&base).ok()?.join(&href).ok()?;
    match fetch(url.as_str()).await {
        Some(Got::Icon(i)) => Some(i),
        _ => None,
    }
}

/// The site's icon as a data URL, or null.
pub async fn icon(host: &str) -> Value {
    if crate::session::LOCKED.load(std::sync::atomic::Ordering::Relaxed)
        || crate::core::try_get()
            .is_some_and(|c| c.store.lock().unwrap().config["privacy"]["localOnly"] == json!(true))
    {
        return Value::Null;
    }
    let host = host.to_lowercase();
    if !valid(&host) {
        return Value::Null;
    }
    if let Some((icon, at)) = KNOWN.lock().unwrap().get_or_insert_with(HashMap::new).get(&host).cloned() {
        if icon.is_some() || at.elapsed() < Duration::from_secs(60) {
            return json!(icon);
        }
    }
    // one fetch per site at a time; everyone asking meanwhile waits for the same answer
    let cell = PENDING.lock().unwrap().get_or_insert_with(HashMap::new).entry(host.clone()).or_default().clone();
    let got = cell.get_or_init(|| load(&host)).await.clone();
    PENDING.lock().unwrap().get_or_insert_with(HashMap::new).remove(&host);
    KNOWN.lock().unwrap().get_or_insert_with(HashMap::new).insert(host, (got.clone(), Instant::now()));
    json!(got)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_host_names_are_fetched() {
        assert!(valid("github.com"));
        assert!(valid("docs.google.com"));
        assert!(!valid("localhost"));
        assert!(!valid("127.0.0.1"));
        assert!(!valid("evil.com/path"));
        assert!(!valid("a..com"));
    }

    #[tokio::test]
    async fn icon_returns_null_when_locked() {
        let was = crate::session::LOCKED.swap(true, std::sync::atomic::Ordering::Relaxed);
        let res = icon("github.com").await;
        crate::session::LOCKED.store(was, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(res, Value::Null);
    }

    #[tokio::test]
    async fn fetch_rejects_oversized_content_length_and_streams() {
        use tokio::io::AsyncWriteExt;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = tokio::io::AsyncReadExt::read(&mut s, &mut buf).await;
                    let req = String::from_utf8_lossy(&buf);
                    if req.contains("/big-cl") {
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 400000\r\nContent-Type: image/png\r\nConnection: close\r\n\r\n").await;
                    } else if req.contains("/big-chunked") {
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: image/png\r\nConnection: close\r\n\r\n").await;
                        let chunk = vec![b'x'; 100_000];
                        for _ in 0..4 {
                            let hdr = format!("{:x}\r\n", chunk.len());
                            if s.write_all(hdr.as_bytes()).await.is_err()
                                || s.write_all(&chunk).await.is_err()
                                || s.write_all(b"\r\n").await.is_err()
                            {
                                break;
                            }
                        }
                        let _ = s.write_all(b"0\r\n\r\n").await;
                    } else {
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Type: image/png\r\nConnection: close\r\n\r\nPNG!").await;
                    }
                });
            }
        });
        assert!(fetch(&format!("http://{addr}/big-cl")).await.is_none());
        assert!(fetch(&format!("http://{addr}/big-chunked")).await.is_none());
        assert!(matches!(fetch(&format!("http://{addr}/small")).await, Some(Got::Icon(_))));
    }
}
