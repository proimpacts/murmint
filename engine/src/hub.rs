use crate::core;
use crate::store::{now_ms, Store};
use crate::usage::{cost_chat, cost_stt, day_key, estimate_tokens};
use serde_json::{json, Value};

#[allow(dead_code)]
pub struct HubError {
    pub message: String,
    pub not_configured: bool,
    pub attempts: Vec<Value>,
}

pub struct ChatOut {
    pub text: String,
    pub r: String,
    pub cost: f64,
    pub tokens_in: u64,
    pub tokens_out: u64,
}
pub struct SttOut {
    pub text: String,
    pub r: String,
    pub cost: f64,
}

pub const HEDGE_CHAT_MS: u64 = 3500;
pub const HEDGE_STT_MS: u64 = 5000;

pub fn available(store: &Store, refs: &[String], task: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for r in refs {
        if r.is_empty() || out.contains(r) {
            continue;
        }
        if let Some((p, m)) = store.find_model(r) {
            if m["task"] == task && store.provider_ready(p["id"].as_str().unwrap_or("")) {
                out.push(r.clone());
            }
        }
    }
    out
}

pub fn cands(store: &Store, refs: &[String], task: &str) -> Vec<Value> {
    available(store, refs, task)
        .iter()
        .filter_map(|r| {
            let (p, m) = store.find_model(r)?;
            let id = p["id"].as_str()?;
            Some(json!({
                "ref": r,
                "type": p["type"],
                "base_url": p["baseUrl"].as_str().unwrap_or(""),
                "account_id": p["accountId"].as_str().unwrap_or(""),
                "model": m["id"],
                "key": store.get_secret(id),
            }))
        })
        .collect()
}

fn lat(store: &mut Store, r: &str, ms: f64) {
    let old = store.latency.get(r).copied();
    store.latency.insert(r.to_string(), old.map_or(ms, |o| o * 0.6 + ms * 0.4));
}

pub fn on_usage(patch: Value) {
    let c = core::get();
    c.store.lock().unwrap().record_usage(&day_key(now_ms()), &patch);
    core::changed("usage");
}

fn engine_error(res: &Value, task: &str) -> HubError {
    let nc = res["not_configured"].as_bool().unwrap_or(false);
    HubError {
        message: if nc {
            if task == "stt" {
                "No speech-recognition model is ready. Add an API key under Models.".into()
            } else {
                "No formatting model is ready.".into()
            }
        } else {
            res["error"].as_str().unwrap_or("Request failed").to_string()
        },
        not_configured: nc,
        attempts: res["attempts"].as_array().cloned().unwrap_or_default(),
    }
}

pub async fn transcribe(
    refs: &[String],
    wav: &[u8],
    duration_sec: f64,
    language: Option<&str>,
    prompt: &str,
) -> Result<SttOut, HubError> {
    use base64::Engine as _;
    let cands = {
        let s = core::get().store.lock().unwrap();
        cands(&s, refs, "stt")
    };
    let res = crate::handle(json!({
        "id": 0, "op": "stt", "cands": cands, "wav_b64": base64::engine::general_purpose::STANDARD.encode(wav),
        "language": language, "prompt": prompt, "hedge_ms": HEDGE_STT_MS, "timeout_ms": 20000
    })).await;
    if res["ok"] != json!(true) {
        return Err(engine_error(&res, "stt"));
    }
    let r = res["ref"].as_str().unwrap_or("").to_string();
    let cost = {
        let mut s = core::get().store.lock().unwrap();
        lat(&mut s, &r, res["ms"].as_f64().unwrap_or(0.0));
        s.find_model(&r).map_or(0.0, |(_, m)| cost_stt(m, duration_sec))
    };
    on_usage(json!({ "model": r, "cost": cost, "modelAudioSec": duration_sec }));
    Ok(SttOut { text: res["text"].as_str().unwrap_or("").to_string(), r, cost })
}

pub async fn chat(
    refs: &[String],
    system: &str,
    user: &str,
    shots: &[(String, String)],
    max_out: u32,
) -> Result<ChatOut, HubError> {
    let cands = {
        let s = core::get().store.lock().unwrap();
        cands(&s, refs, "chat")
    };
    let res = crate::handle(json!({
        "id": 0, "op": "chat", "cands": cands, "system": system, "user": user, "shots": crate::prompt::shots_json(shots),
        "max_out": max_out, "hedge_ms": HEDGE_CHAT_MS, "timeout_ms": 15000
    })).await;
    if res["ok"] != json!(true) {
        return Err(engine_error(&res, "chat"));
    }
    let r = res["ref"].as_str().unwrap_or("").to_string();
    let text = res["text"].as_str().unwrap_or("").to_string();
    let tin = res["tokens_in"].as_u64().unwrap_or_else(|| estimate_tokens(&format!("{system}{user}")));
    let tout = res["tokens_out"].as_u64().unwrap_or_else(|| estimate_tokens(&text));
    let cost = {
        let mut s = core::get().store.lock().unwrap();
        lat(&mut s, &r, res["ms"].as_f64().unwrap_or(0.0));
        s.find_model(&r).map_or(0.0, |(_, m)| cost_chat(m, tin as f64, tout as f64))
    };
    on_usage(json!({ "model": r, "cost": cost, "tokensIn": tin, "tokensOut": tout }));
    Ok(ChatOut { text, r, cost, tokens_in: tin, tokens_out: tout })
}

pub fn prewarm(r: &str) {
    let url = {
        let s = core::get().store.lock().unwrap();
        s.find_model(r).map(|(p, _)| {
            if p["type"] == "cloudflare" {
                "https://api.cloudflare.com/".to_string()
            } else {
                p["baseUrl"].as_str().unwrap_or("").to_string()
            }
        })
    };
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        tokio::spawn(async move {
            let _ = crate::handle(json!({ "id": 0, "op": "prewarm", "url": url })).await;
        });
    }
}

pub async fn test(provider_id: &str) -> Value {
    let (provider, key) = {
        let s = core::get().store.lock().unwrap();
        (s.provider(provider_id).cloned(), s.get_secret(provider_id))
    };
    let Some(p) = provider else { return json!({ "ok": false, "error": "Unknown provider" }) };
    if key.is_empty() {
        return json!({ "ok": false, "error": "No API key saved" });
    }
    if p["type"] == "cloudflare" && p["accountId"].as_str().unwrap_or("").is_empty() {
        return json!({ "ok": false, "error": "Add your Cloudflare Account ID" });
    }
    let t0 = std::time::Instant::now();
    let cand = json!({
        "ref": provider_id,
        "type": p["type"],
        "base_url": p["baseUrl"].as_str().unwrap_or(""),
        "account_id": p["accountId"].as_str().unwrap_or(""),
        "key": key,
        "model": "",
    });
    // Cloudflare: ask Cloudflare whether the token is valid, so a speech-only setup tests too
    let res = if p["type"] == "cloudflare" {
        let account = p["accountId"].as_str().unwrap_or("").trim().to_string();
        let r = crate::CLIENT
            .get(format!("https://api.cloudflare.com/client/v4/accounts/{account}/ai/models/search?per_page=1"))
            .bearer_auth(&key)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await;
        match r {
            Ok(r) if r.status().is_success() => json!({ "ok": true }),
            Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => {
                json!({ "ok": false, "error": "Cloudflare rejected the token, or it isn't allowed to use Workers AI" })
            }
            Ok(r) => json!({ "ok": false, "error": format!("Cloudflare answered {}", r.status().as_u16()) }),
            Err(e) => json!({ "ok": false, "error": e.without_url().to_string() }),
        }
    } else {
        crate::handle(json!({ "id": 0, "op": "ping", "cands": [cand] })).await
    };
    if res["ok"] == json!(true) {
        json!({ "ok": true, "ms": res["ms"].as_u64().unwrap_or(t0.elapsed().as_millis() as u64) })
    } else {
        json!({ "ok": false, "error": res["error"] })
    }
}
