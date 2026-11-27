#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]
mod accent;
mod account;
mod agent_chat;
mod clipboard;
mod core;
mod denoise;
mod formatter;
mod hub;
mod insertion;
mod ipc;
mod learn;
mod murmint1;
mod native;
mod presets;
mod profiles;
mod prompt;
mod route;
mod session;
#[cfg(all(test, local_tests))]
mod session_tests;
mod site_icons;
mod store;
mod text;
mod updater;
mod usage;
mod vocab;
mod vocab_ai;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::Instant;

mod rt;

#[derive(Deserialize, Clone, Debug, Default)]
pub(crate) struct Cand {
    #[serde(rename = "ref", default)]
    reference: String,
    #[serde(rename = "type", default)]
    ptype: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    account_id: String,
    model: String,
    #[serde(default)]
    key: String,
}

#[derive(Debug, Clone)]
struct PErr {
    message: String,
    kind: &'static str,
}

#[derive(Debug)]
enum Out {
    Stt(String),
    Chat { text: String, tin: Option<u64>, tout: Option<u64> },
}

type Fut = Pin<Box<dyn Future<Output = Result<Out, PErr>> + Send>>;

pub(crate) static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_idle_timeout(Duration::from_secs(120))
        .pool_max_idle_per_host(8)
        .tcp_nodelay(true)
        .tcp_keepalive(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(8))
        .user_agent("Murmint")
        .build()
        .expect("http client")
});

static OUT: std::sync::OnceLock<mpsc::UnboundedSender<String>> = std::sync::OnceLock::new();

pub(crate) fn say(v: Value) {
    if let Some(tx) = OUT.get() {
        let _ = tx.send(v.to_string());
    }
}

static REJECTED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn friendly_cf(msg: &str) -> String {
    let m = msg.to_lowercase();
    if ["could not route", "no route for that uri", "7003", "7000", "invalid object identifier"]
        .iter()
        .any(|k| m.contains(k))
    {
        return "Cloudflare could not find that account. Check the Account ID: 32 characters, from the dashboard under Workers AI > Use REST API.".into();
    }
    if [
        "authentication",
        "unauthorized",
        "unauthorised",
        "forbidden",
        "invalid api token",
        "10000",
        "9109",
        "9106",
        "not authorized",
        "permission",
    ]
    .iter()
    .any(|k| m.contains(k))
    {
        return "Cloudflare rejected the token. It must be an API Token (not the Global API Key) with the Workers AI permission, created for the same account as the Account ID.".into();
    }
    msg.to_string()
}

fn classify(status: u16, body: &str) -> PErr {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["errors"][0]["message"].as_str())
                .or_else(|| v["message"].as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| format!("HTTP {status}: {}", body.chars().take(160).collect::<String>()));
    match status {
        401 | 403 => PErr { message: format!("Authentication failed: {msg}"), kind: "auth" },
        429 => PErr { message: format!("Rate limited: {msg}"), kind: "rate" },
        500..=599 => PErr { message: format!("Provider error: {msg}"), kind: "server" },
        _ => PErr { message: msg, kind: "bad-request" },
    }
}

fn net_err(e: reqwest::Error) -> PErr {
    PErr {
        message: if e.is_timeout() {
            "Request timed out".into()
        } else {
            format!("Network error: {}", e.without_url())
        },
        kind: "network",
    }
}

fn cf_base(account: &str) -> String {
    format!("https://api.cloudflare.com/client/v4/accounts/{}/ai", account.trim())
}
fn on_device_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url.trim()) else {
        return false;
    };
    matches!(
        parsed.host_str().map(str::to_ascii_lowercase).as_deref(),
        Some("127.0.0.1" | "localhost" | "[::1]")
    )
}
fn local_only_enabled() -> bool {
    core::try_get().is_some_and(|c| c.store.lock().unwrap().config["privacy"]["localOnly"] == json!(true))
}
fn check_local_only(url: &str) -> Result<(), PErr> {
    if local_only_enabled() && !on_device_url(url) {
        return Err(PErr {
            message: "Blocked non-local request while local-only mode is enabled.".into(),
            kind: "error",
        });
    }
    Ok(())
}

// Models on this computer run on the CPU, so they get more time than a cloud API.
fn on_device(c: &Cand, timeout: Duration) -> Duration {
    if on_device_url(&c.base_url) {
        timeout * 3
    } else {
        timeout
    }
}

fn chat_base(c: &Cand) -> String {
    if c.ptype == "anthropic" && c.base_url.trim().is_empty() {
        return "https://api.anthropic.com/v1".to_string();
    }
    if c.ptype == "cloudflare" {
        format!("{}/v1", cf_base(&c.account_id))
    } else {
        c.base_url.trim_end_matches('/').to_string()
    }
}

async fn read(req: reqwest::RequestBuilder) -> Result<String, PErr> {
    let resp = req.send().await.map_err(net_err)?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(net_err)?;
    if !(200..300).contains(&status) {
        return Err(classify(status, &body));
    }
    Ok(body)
}

fn polish(c: &Cand, mut e: PErr) -> PErr {
    if c.ptype == "cloudflare" {
        e.message = friendly_cf(&e.message);
    }
    e
}

async fn stt_once(
    c: &Cand,
    wav: &[u8],
    language: Option<&str>,
    prompt: &str,
    timeout: Duration,
) -> Result<String, PErr> {
    let lang = language.filter(|l| {
        (2..=3).contains(&l.len()) && *l != "auto" && l.chars().all(|ch| ch.is_ascii_alphabetic())
    });
    if c.ptype == "cloudflare" {
        let url = format!("{}/run/{}", cf_base(&c.account_id), c.model);
        check_local_only(&url)?;
        let req = CLIENT.post(url).bearer_auth(&c.key).timeout(timeout);
        let req = if c.model.contains("whisper-large-v3-turbo") {
            let audio = base64::engine::general_purpose::STANDARD.encode(wav);
            let mut body = json!({ "audio": audio, "task": "transcribe", "vad_filter": true });
            if let Some(l) = lang {
                body["language"] = json!(l.to_lowercase());
            }
            if !prompt.is_empty() {
                body["initial_prompt"] = json!(prompt.chars().take(800).collect::<String>());
            }
            req.json(&body)
        } else {
            req.header("content-type", "application/octet-stream").body(wav.to_vec())
        };
        let body = read(req).await.map_err(|e| polish(c, e))?;
        let v: Value =
            serde_json::from_str(&body).map_err(|e| PErr { message: e.to_string(), kind: "error" })?;
        if v["success"] == json!(false) {
            return Err(PErr {
                message: friendly_cf(v["errors"][0]["message"].as_str().unwrap_or("Transcription failed")),
                kind: "error",
            });
        }
        let text = v["result"]["text"].as_str().ok_or_else(|| PErr {
            message: "Speech response missing transcript field".into(),
            kind: "error",
        })?;
        return Ok(text.trim().to_string());
    }
    let url = format!("{}/audio/transcriptions", chat_base(c));
    check_local_only(&url)?;
    let part =
        reqwest::multipart::Part::bytes(wav.to_vec()).file_name("audio.wav").mime_str("audio/wav").unwrap();
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", c.model.clone())
        .text("response_format", "json");
    if let Some(l) = lang {
        form = form.text("language", l.to_lowercase());
    }
    if !prompt.is_empty() {
        form = form.text("prompt", prompt.chars().take(800).collect::<String>());
    }
    let mut req = CLIENT.post(url).multipart(form).timeout(timeout);
    if !c.key.is_empty() {
        req = req.bearer_auth(&c.key);
    }
    let body = read(req).await?;
    let v: Value = serde_json::from_str(&body).map_err(|e| PErr { message: e.to_string(), kind: "error" })?;
    let text = v["text"].as_str().ok_or_else(|| PErr {
        message: "Speech response missing transcript field".into(),
        kind: "error",
    })?;
    Ok(text.trim().to_string())
}

async fn anthropic_chat(
    c: &Cand,
    system: &str,
    user: &str,
    shots: &[(String, String)],
    max_out: u32,
    timeout: Duration,
) -> Result<(String, Option<u64>, Option<u64>), PErr> {
    let url = format!("{}/messages", chat_base(c));
    check_local_only(&url)?;
    let mut messages = vec![];
    for (u, a) in shots {
        messages.push(json!({"role": "user", "content": u}));
        messages.push(json!({"role": "assistant", "content": a}));
    }
    messages.push(json!({"role": "user", "content": user}));
    let mut body = json!({
        "model": c.model,
        "max_tokens": max_out.max(1500),
        "system": system,
        "messages": messages,
        "output_config": { "effort": "low" },
    });
    for attempt in 0..2 {
        let req = CLIENT
            .post(&url)
            .header("x-api-key", &c.key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .timeout(timeout);
        match read(req).await {
            Ok(text) => {
                let v: Value = serde_json::from_str(&text)
                    .map_err(|e| PErr { message: e.to_string(), kind: "error" })?;
                let out: String = v["content"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter(|b| b["type"] == "text")
                            .filter_map(|b| b["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default();
                if out.trim().is_empty() {
                    return Err(PErr { message: "Model returned no text".into(), kind: "error" });
                }
                return Ok((
                    out.trim().to_string(),
                    v["usage"]["input_tokens"].as_u64(),
                    v["usage"]["output_tokens"].as_u64(),
                ));
            }
            Err(e)
                if attempt == 0
                    && e.kind == "bad-request"
                    && (e.message.to_lowercase().contains("effort")
                        || e.message.to_lowercase().contains("output_config")) =>
            {
                body.as_object_mut().map(|o| o.remove("output_config"));
            }
            Err(e) => return Err(polish(c, e)),
        }
    }
    Err(PErr { message: "Model request failed".into(), kind: "error" })
}

async fn chat_once(
    c: &Cand,
    system: &str,
    user: &str,
    shots: &[(String, String)],
    max_out: u32,
    timeout: Duration,
) -> Result<(String, Option<u64>, Option<u64>), PErr> {
    if c.ptype == "anthropic" {
        return anthropic_chat(c, system, user, shots, max_out, timeout).await;
    }
    let base = chat_base(c);
    let url = format!("{base}/chat/completions");
    check_local_only(&url)?;
    let m = &c.model;
    let reasoning = c.ptype == "openai"
        && (m.starts_with("gpt-5")
            || (m.starts_with('o') && m.chars().nth(1).map(|ch| ch.is_ascii_digit()).unwrap_or(false)));
    let mut params: Vec<(&str, Value)> = vec![];
    if reasoning {
        params.push(("reasoning_effort", json!("minimal")));
    } else {
        params.push(("temperature", json!(0)));
    }
    if c.ptype != "openai" && max_out > 0 {
        params.push(("max_tokens", json!(max_out)));
    }
    let rej = |p: &str| format!("{base}|{m}|{p}");
    params.retain(|(k, _)| !REJECTED.lock().unwrap().contains(&rej(k)));

    let mut messages = vec![json!({"role": "system", "content": system})];
    for (u, a) in shots {
        messages.push(json!({"role": "user", "content": u}));
        messages.push(json!({"role": "assistant", "content": a}));
    }
    messages.push(json!({"role": "user", "content": user}));

    for attempt in 0..2 {
        let mut body = json!({ "model": m, "messages": messages });
        for (k, v) in &params {
            body[*k] = v.clone();
        }
        let mut req = CLIENT.post(format!("{base}/chat/completions")).json(&body).timeout(timeout);
        if !c.key.is_empty() {
            req = req.bearer_auth(&c.key);
        }
        match read(req).await {
            Ok(text) => {
                let v: Value = serde_json::from_str(&text)
                    .map_err(|e| PErr { message: e.to_string(), kind: "error" })?;
                let out = v["choices"][0]["message"]["content"]
                    .as_str()
                    .or_else(|| v["result"]["response"].as_str())
                    .unwrap_or("")
                    .to_string();
                if out.trim().is_empty() {
                    return Err(PErr { message: "Model returned no text".into(), kind: "error" });
                }
                return Ok((
                    out,
                    v["usage"]["prompt_tokens"].as_u64(),
                    v["usage"]["completion_tokens"].as_u64(),
                ));
            }
            Err(e)
                if attempt == 0
                    && e.kind == "bad-request"
                    && !params.is_empty()
                    && [
                        "temperature",
                        "reasoning",
                        "unsupported",
                        "not supported",
                        "unknown",
                        "max_tokens",
                        "max_completion",
                    ]
                    .iter()
                    .any(|k| e.message.to_lowercase().contains(k)) =>
            {
                for (k, _) in &params {
                    REJECTED.lock().unwrap().insert(rej(k));
                }
                params.clear();
            }
            Err(e) => return Err(polish(c, e)),
        }
    }
    Err(PErr { message: "Model request failed".into(), kind: "error" })
}

async fn race(
    cands: Vec<Cand>,
    hedge: Duration,
    work: impl Fn(Cand) -> Fut,
) -> Result<(Out, Cand, u128), Vec<Value>> {
    let mut set: JoinSet<(usize, Result<Out, PErr>, u128)> = JoinSet::new();
    let (mut next, mut attempts) = (0usize, Vec::<Value>::new());
    let mut deadline = Instant::now() + hedge;

    macro_rules! launch {
        () => {{
            let idx = next;
            next += 1;
            let fut = work(cands[idx].clone());
            set.spawn(async move {
                let t = std::time::Instant::now();
                let r = fut.await;
                (idx, r, t.elapsed().as_millis())
            });
            deadline = Instant::now() + hedge;
        }};
    }

    loop {
        if set.is_empty() {
            if next >= cands.len() {
                return Err(attempts);
            }
            launch!();
        }
        tokio::select! {
            res = set.join_next() => {
                if let Some(Ok((i, r, ms))) = res {
                    match r {
                        Ok(out) => { set.abort_all(); return Ok((out, cands[i].clone(), ms)); }
                        Err(e) => {
                            attempts.push(json!({ "ref": cands[i].reference, "error": e.message, "kind": e.kind }));
                            if next < cands.len() { launch!(); }
                        }
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline), if next < cands.len() => { launch!(); }
        }
    }
}

pub(crate) fn parse_cands(v: &Value) -> Vec<Cand> {
    v.as_array()
        .map(|a| a.iter().filter_map(|c| serde_json::from_value::<Cand>(c.clone()).ok()).collect())
        .unwrap_or_default()
}

fn failure(id: &Value, prefix: &str, attempts: Vec<Value>, none: bool) -> Value {
    let joined = attempts.iter().filter_map(|a| a["error"].as_str()).collect::<Vec<_>>().join(" → ");
    let error = if none { "No model is ready.".to_string() } else { format!("{prefix}: {joined}") };
    json!({ "id": id, "ok": false, "error": error, "attempts": attempts, "not_configured": none })
}

pub(crate) async fn handle(req: Value) -> Value {
    let id = req["id"].clone();
    match req["op"].as_str().unwrap_or("") {
        "hello" => {
            json!({ "id": id, "ok": true, "engine": "murmint-engine", "version": env!("CARGO_PKG_VERSION") })
        }

        "text_local" => {
            let mut r = text::op_local(&req);
            r["id"] = id;
            r["ok"] = json!(true);
            r
        }

        "prewarm" => {
            let url = req["url"].as_str().unwrap_or("").to_string();
            if !url.is_empty() {
                if let Err(e) = check_local_only(&url) {
                    return json!({ "id": id, "ok": false, "error": e.message });
                }
                let _ = CLIENT.head(url).timeout(Duration::from_secs(4)).send().await;
            }
            json!({ "id": id, "ok": true })
        }

        "ping" => {
            let cands = parse_cands(&req["cands"]);
            let Some(c) = cands.first() else {
                return json!({ "id": id, "ok": false, "error": "No provider given" });
            };
            let url = format!("{}/models", chat_base(c));
            if let Err(e) = check_local_only(&url) {
                return json!({ "id": id, "ok": false, "error": e.message });
            }
            let t = std::time::Instant::now();
            let mut r = CLIENT.get(url).timeout(Duration::from_secs(10));
            if c.ptype == "anthropic" {
                r = r.header("x-api-key", &c.key).header("anthropic-version", "2023-06-01");
            } else if !c.key.is_empty() {
                r = r.bearer_auth(&c.key);
            }
            match read(r).await {
                Ok(_) => json!({ "id": id, "ok": true, "ms": t.elapsed().as_millis() as u64 }),
                Err(e) => json!({ "id": id, "ok": false, "error": polish(c, e).message }),
            }
        }

        "stt" => {
            let cands = parse_cands(&req["cands"]);
            if cands.is_empty() {
                return failure(&id, "Speech recognition failed", vec![], true);
            }
            let Ok(wav) =
                base64::engine::general_purpose::STANDARD.decode(req["wav_b64"].as_str().unwrap_or(""))
            else {
                return json!({ "id": id, "ok": false, "error": "bad audio payload", "transport": true });
            };
            let wav = std::sync::Arc::new(wav);
            let lang = req["language"].as_str().map(|s| s.to_string());
            let prompt = req["prompt"].as_str().unwrap_or("").to_string();
            let timeout = Duration::from_millis(req["timeout_ms"].as_u64().unwrap_or(20_000));
            let hedge = Duration::from_millis(req["hedge_ms"].as_u64().unwrap_or(5_000));
            let work = move |c: Cand| -> Fut {
                let (wav, lang, prompt) = (wav.clone(), lang.clone(), prompt.clone());
                Box::pin(async move {
                    stt_once(&c, &wav, lang.as_deref(), &prompt, on_device(&c, timeout))
                        .await
                        .map(Out::Stt)
                        .map_err(|e| polish(&c, e))
                })
            };
            match race(cands, hedge, work).await {
                Ok((Out::Stt(text), c, ms)) => {
                    json!({ "id": id, "ok": true, "text": text, "ref": c.reference, "ms": ms as u64 })
                }
                Ok(_) => json!({ "id": id, "ok": false, "error": "unexpected result" }),
                Err(attempts) => failure(&id, "Speech recognition failed", attempts, false),
            }
        }

        "chat" => {
            let cands = parse_cands(&req["cands"]);
            if cands.is_empty() {
                return failure(&id, "Formatting failed", vec![], true);
            }
            let system = req["system"].as_str().unwrap_or("").to_string();
            let user = req["user"].as_str().unwrap_or("").to_string();
            let shots: Vec<(String, String)> = req["shots"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|s| {
                            (
                                s["user"].as_str().unwrap_or("").to_string(),
                                s["assistant"].as_str().unwrap_or("").to_string(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let max_out = req["max_out"].as_u64().unwrap_or(0) as u32;
            let timeout = Duration::from_millis(req["timeout_ms"].as_u64().unwrap_or(15_000));
            let hedge = Duration::from_millis(req["hedge_ms"].as_u64().unwrap_or(3_500));
            let work = move |c: Cand| -> Fut {
                let (system, user, shots) = (system.clone(), user.clone(), shots.clone());
                Box::pin(async move {
                    chat_once(&c, &system, &user, &shots, max_out, on_device(&c, timeout))
                        .await
                        .map(|(text, tin, tout)| Out::Chat { text, tin, tout })
                })
            };
            match race(cands, hedge, work).await {
                Ok((Out::Chat { text, tin, tout }, c, ms)) => {
                    json!({
                        "id": id,
                        "ok": true,
                        "text": text,
                        "ref": c.reference,
                        "ms": ms as u64,
                        "tokens_in": tin,
                        "tokens_out": tout,
                    })
                }
                Ok(_) => json!({ "id": id, "ok": false, "error": "unexpected result" }),
                Err(attempts) => failure(&id, "Formatting failed", attempts, false),
            }
        }

        other => {
            if other == "native_combos" {
                native::start_hotkeys(|v| say(v));
            }
            match native::handle(other, &req).await {
                Some(r) => r,
                None => {
                    json!({ "id": id, "ok": false, "error": format!("unknown op '{other}'"), "transport": true })
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let _ = OUT.set(tx.clone());
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(line) = rx.recv().await {
            if out.write_all(line.as_bytes()).await.is_err()
                || out.write_all(b"\n").await.is_err()
                || out.flush().await.is_err()
            {
                break;
            }
        }
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        if line.contains("\"op\":\"send\"") {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let args: Vec<Value> = v["args"].as_array().cloned().unwrap_or_default();
                ipc::send(v["ch"].as_str().unwrap_or(""), &args);
            }
            continue;
        }
        if line.contains("\"op\":\"rt_") {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let stream = v["stream"].as_u64().unwrap_or(0);
                match v["op"].as_str().unwrap_or("") {
                    "rt_open" => {
                        let strings = |x: &Value| -> Vec<String> {
                            x.as_array()
                                .map(|a| a.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
                                .unwrap_or_default()
                        };
                        let reply = match parse_cands(&v["cands"]).into_iter().next() {
                            Some(cand) => {
                                rt::open(
                                    rt::OpenReq {
                                        stream,
                                        cand,
                                        languages: strings(&v["languages"]),
                                        keywords: strings(&v["keywords"]),
                                        hint: v["hint"].as_str().unwrap_or("").to_string(),
                                    },
                                    std::sync::Arc::new(say),
                                );
                                json!({ "id": v["id"], "ok": true })
                            }
                            None => json!({ "id": v["id"], "ok": false, "error": "No live model given" }),
                        };
                        let _ = tx.send(reply.to_string());
                    }
                    "rt_audio" => {
                        if let Ok(pcm) = base64::engine::general_purpose::STANDARD
                            .decode(v["pcm_b64"].as_str().unwrap_or(""))
                        {
                            rt::cmd(stream, rt::Cmd::Audio(pcm));
                        }
                    }
                    "rt_commit" => rt::cmd(
                        stream,
                        rt::Cmd::Commit {
                            last: v["last"].as_bool().unwrap_or(false),
                            gap_ms: v["gap_ms"].as_u64(),
                        },
                    ),
                    "rt_close" => rt::cmd(stream, rt::Cmd::Close),
                    _ => {}
                }
            }
            continue;
        }
        if line.contains("\"op\":\"ui\"")
            || line.contains("\"op\":\"init\"")
            || line.contains("\"op\":\"quit\"")
        {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let id = v["id"].clone();
                match v["op"].as_str().unwrap_or("") {
                    "init" => {
                        let dir = std::path::PathBuf::from(v["dir"].as_str().unwrap_or("."));
                        core::init(&dir, v["version"].as_str().unwrap_or(""));
                        session::LOCKED.store(v["locked"] == json!(true), std::sync::atomic::Ordering::Relaxed);
                        session::init();
                        native::start_hotkeys(session::on_hotkey_event);
                        clipboard::start(|ch, v| core::push(ch, "shell", v));
                        tokio::spawn(murmint1::boot());
                        static UPDATES: std::sync::Once = std::sync::Once::new();
                        UPDATES.call_once(updater::start);
                        core::apply_hotkeys();
                        *core::get().helper_ready.lock().unwrap() = true;
                        tokio::spawn(async {
                            let mut t = tokio::time::interval(Duration::from_millis(300));
                            let mut ticks: u32 = 0;
                            loop {
                                t.tick().await;
                                ticks = ticks.wrapping_add(1);
                                let check_prune = ticks % 200 == 0;
                                let mut pruned = false;
                                if let Some(c) = core::try_get() {
                                    let mut s = c.store.lock().unwrap();
                                    if check_prune {
                                        let before = s.history.len();
                                        s.prune_history();
                                        pruned = before != s.history.len();
                                    }
                                    if s.has_unsaved() {
                                        s.flush();
                                    }
                                }
                                if pruned {
                                    core::changed("history");
                                }
                            }
                        });
                        core::push("state:changed", "all", core::state_for_renderer());
                        let _ = tx.send(json!({ "id": id, "ok": true }).to_string());
                    }
                    "ui" => {
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let args: Vec<Value> = v["args"].as_array().cloned().unwrap_or_default();
                            let value = ipc::ui(v["ch"].as_str().unwrap_or(""), &args).await;
                            let _ = tx.send(json!({ "id": id, "ok": true, "value": value }).to_string());
                        });
                    }
                    _ => {
                        murmint1::stop();
                        if let Some(c) = core::try_get() {
                            c.store.lock().unwrap().flush();
                        }
                        let _ = tx.send(json!({ "id": id, "ok": true }).to_string());
                    }
                }
            }
            continue;
        }
        let tx = tx.clone();
        tokio::spawn(async move {
            let resp = match serde_json::from_str::<Value>(&line) {
                Ok(req) => handle(req).await,
                Err(e) => {
                    json!({ "id": null, "ok": false, "error": format!("bad json: {e}"), "transport": true })
                }
            };
            let _ = tx.send(resp.to_string());
        });
    }
    if let Some(c) = core::try_get() {
        c.store.lock().unwrap().flush();
    }
    drop(tx);
    let _ = tokio::time::timeout(Duration::from_millis(300), writer).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    type Log = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    fn serve<F: Fn(&str, &[u8]) -> (u16, String, u64) + Send + Sync + 'static>(handler: F) -> (String, Log) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}/v1", l.local_addr().unwrap().port());
        let log: Log = Arc::new(Mutex::new(vec![]));
        let (log2, handler) = (log.clone(), Arc::new(handler));
        std::thread::spawn(move || {
            for stream in l.incoming() {
                let Ok(mut s) = stream else { continue };
                let (log3, handler) = (log2.clone(), handler.clone());
                std::thread::spawn(move || {
                    let (mut buf, mut tmp) = (Vec::new(), [0u8; 16384]);
                    let (path, body) = loop {
                        let n = s.read(&mut tmp).unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                            let clen = head
                                .lines()
                                .find_map(|l| {
                                    l.to_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            let path = head
                                .lines()
                                .next()
                                .unwrap_or("")
                                .split_whitespace()
                                .nth(1)
                                .unwrap_or("")
                                .to_string();
                            while buf.len() < pos + 4 + clen {
                                let n = s.read(&mut tmp).unwrap_or(0);
                                if n == 0 {
                                    break;
                                }
                                buf.extend_from_slice(&tmp[..n]);
                            }
                            break (path, buf[pos + 4..].to_vec());
                        }
                    };
                    let (status, resp, delay) = handler(&path, &body);
                    log3.lock().unwrap().push((path, body));
                    std::thread::sleep(Duration::from_millis(delay));
                    let _ = write!(s, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}", resp.len());
                });
            }
        });
        (base, log)
    }

    fn cand(base: &str, reference: &str) -> Value {
        json!({ "ref": reference, "type": "compatible", "base_url": base, "model": "m", "key": "k" })
    }
    #[test]
    fn models_on_this_computer_get_more_time() {
        let c = |base: &str| serde_json::from_value::<Cand>(cand(base, "x::m")).unwrap();
        let t = Duration::from_secs(15);
        assert_eq!(on_device(&c("http://127.0.0.1:5051/v1"), t), t * 3);
        assert_eq!(on_device(&c("http://localhost:11434/v1"), t), t * 3);
        assert_eq!(on_device(&c("http://[::1]:11434/v1"), t), t * 3);
        assert_eq!(on_device(&c("https://api.openai.com/v1"), t), t);
        assert_eq!(on_device(&c("https://localhost.example.com/v1"), t), t);
    }

    #[tokio::test]
    async fn malformed_stt_response_loses_to_healthy_fallback_and_empty_text_is_preserved() {
        let (bad, _) = serve(|_, _| (200, r#"{"unexpected":1}"#.into(), 0));
        let (good, _) = serve(|_, _| (200, r#"{"text":"hello"}"#.into(), 0));
        let r = handle(json!({
            "id": 20, "op": "stt", "cands": [cand(&bad, "bad::m"), cand(&good, "good::m")],
            "wav_b64": "QUJD", "hedge_ms": 5000
        }))
        .await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["text"], "hello");
        assert_eq!(r["ref"], "good::m");

        let r_all_bad = handle(json!({
            "id": 21, "op": "stt", "cands": [cand(&bad, "bad::m")], "wav_b64": "QUJD"
        }))
        .await;
        assert_eq!(r_all_bad["ok"], false, "{r_all_bad}");

        let (empty, _) = serve(|_, _| (200, r#"{"text":""}"#.into(), 0));
        let r_empty = handle(json!({
            "id": 22, "op": "stt", "cands": [cand(&empty, "empty::m")], "wav_b64": "QUJD"
        }))
        .await;
        assert_eq!(r_empty["ok"], true, "{r_empty}");
        assert_eq!(r_empty["text"], "");
    }

    const OK: &str = r#"{"choices":[{"message":{"content":"cleaned text"}}],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#;

    #[tokio::test]
    async fn chat_sends_examples_and_temperature_zero_and_reports_tokens() {
        let (base, log) = serve(|_, _| (200, OK.into(), 0));
        let r = handle(
            json!({ "id": 1, "op": "chat", "cands": [cand(&base, "a::m")], "system": "SYS", "user": "USER",
            "shots": [{ "user": "u1", "assistant": "a1" }], "max_out": 100 }),
        )
        .await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            (
                r["text"].as_str().unwrap(),
                r["ref"].as_str().unwrap(),
                r["tokens_in"].as_u64(),
                r["tokens_out"].as_u64()
            ),
            ("cleaned text", "a::m", Some(12), Some(3))
        );
        let body: Value = serde_json::from_slice(&log.lock().unwrap()[0].1).unwrap();
        assert_eq!(body["temperature"], 0);
        assert_eq!(body["max_tokens"], 100);
        let roles: Vec<&str> =
            body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert_eq!(body["messages"][3]["content"], "USER");
    }

    const CLAUDE_OK: &str = r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"Cleaned by Claude."}],"usage":{"input_tokens":21,"output_tokens":6}}"#;

    #[tokio::test]
    async fn claude_uses_the_messages_api_without_sampling_settings() {
        let (base, log) = serve(|_, _| (200, CLAUDE_OK.into(), 0));
        let c = json!({ "ref": "anthropic::claude-haiku-5-5", "type": "anthropic", "base_url": base, "model": "claude-haiku-5-5", "key": "k" });
        let r = handle(json!({ "id": 1, "op": "chat", "cands": [c], "system": "SYS", "user": "USER", "shots": [{ "user": "u1", "assistant": "a1" }], "max_out": 100 })).await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(
            (r["text"].as_str().unwrap(), r["tokens_in"].as_u64(), r["tokens_out"].as_u64()),
            ("Cleaned by Claude.", Some(21), Some(6)),
            "thinking blocks are skipped"
        );
        let (path, raw) = log.lock().unwrap()[0].clone();
        assert!(path.ends_with("/v1/messages"), "{path}");
        let body: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(body["system"], "SYS", "system is its own field");
        assert!(body.get("temperature").is_none(), "Claude 5.5 rejects non default sampling settings");
        assert_eq!(body["output_config"]["effort"], "low");
        assert!(body["max_tokens"].as_u64().unwrap() >= 1500, "room for thinking tokens");
        let roles: Vec<&str> =
            body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["user", "assistant", "user"]);
    }

    #[tokio::test]
    async fn claude_retries_without_effort_for_a_model_that_has_none() {
        let (base, log) = serve(|_, body| {
            if String::from_utf8_lossy(body).contains("output_config") {
                (400, r#"{"error":{"message":"effort is not supported for this model"}}"#.into(), 0)
            } else {
                (200, CLAUDE_OK.into(), 0)
            }
        });
        let c = json!({ "ref": "anthropic::old", "type": "anthropic", "base_url": base, "model": "old", "key": "k" });
        let r = handle(json!({ "id": 2, "op": "chat", "cands": [c], "system": "s", "user": "u" })).await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_failing_model_starts_the_next_one_immediately() {
        let (bad, _) = serve(|_, _| (500, r#"{"error":{"message":"boom"}}"#.into(), 0));
        let (good, _) = serve(|_, _| (200, OK.into(), 0));
        let t = std::time::Instant::now();
        let r = handle(json!({ "id": 2, "op": "chat", "cands": [cand(&bad, "bad::m"), cand(&good, "good::m")], "system": "s", "user": "u", "hedge_ms": 5000 })).await;
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["ref"], "good::m");
        assert_eq!(r["attempts"], Value::Null);
        assert!(
            t.elapsed() < Duration::from_millis(1500),
            "must not wait for the hedge timer after a failure"
        );
    }

    #[tokio::test]
    async fn a_slow_model_is_hedged_with_the_next_one() {
        let (slow, _) = serve(|_, _| (200, OK.replace("cleaned text", "from slow"), 1500));
        let (fast, _) = serve(|_, _| (200, OK.replace("cleaned text", "from fast"), 0));
        let t = std::time::Instant::now();
        let r = handle(json!({ "id": 3, "op": "chat", "cands": [cand(&slow, "slow::m"), cand(&fast, "fast::m")], "system": "s", "user": "u", "hedge_ms": 150 })).await;
        assert_eq!(r["text"], "from fast", "{r}");
        assert!(t.elapsed() < Duration::from_millis(1000), "{:?}", t.elapsed());
    }

    #[tokio::test]
    async fn all_failing_reports_every_attempt_in_order() {
        let (a, _) = serve(|_, _| (401, r#"{"error":{"message":"bad key"}}"#.into(), 0));
        let (b, _) = serve(|_, _| (429, r#"{"error":{"message":"slow down"}}"#.into(), 0));
        let r = handle(json!({ "id": 4, "op": "chat", "cands": [cand(&a, "a::m"), cand(&b, "b::m")], "system": "s", "user": "u" })).await;
        assert_eq!(r["ok"], false);
        assert_eq!(r["not_configured"], false);
        let kinds: Vec<&str> =
            r["attempts"].as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["auth", "rate"]);
        assert!(
            r["error"].as_str().unwrap().contains("Authentication failed: bad key → Rate limited: slow down"),
            "{r}"
        );
    }

    #[tokio::test]
    async fn no_candidates_means_not_configured() {
        for op in ["chat", "stt"] {
            let r = handle(json!({ "id": 5, "op": op, "cands": [] })).await;
            assert_eq!((r["ok"].as_bool(), r["not_configured"].as_bool()), (Some(false), Some(true)));
        }
    }

    #[tokio::test]
    async fn unsupported_optional_params_are_dropped_and_remembered() {
        let calls = Arc::new(Mutex::new(0));
        let c2 = calls.clone();
        let (base, log) = serve(move |_, body| {
            *c2.lock().unwrap() += 1;
            let b: Value = serde_json::from_slice(body).unwrap_or_default();
            if b.get("temperature").is_some() {
                (400, r#"{"error":{"message":"Unsupported parameter: 'temperature'"}}"#.into(), 0)
            } else {
                (200, OK.into(), 0)
            }
        });
        let req =
            json!({ "id": 6, "op": "chat", "cands": [cand(&base, "x::m")], "system": "s", "user": "u" });
        assert_eq!(handle(req.clone()).await["ok"], true);
        assert_eq!(*calls.lock().unwrap(), 2, "one rejected attempt, one retry without the parameter");
        assert_eq!(handle(req).await["ok"], true);
        assert_eq!(*calls.lock().unwrap(), 3, "the next request goes out clean the first time");
        assert!(serde_json::from_slice::<Value>(&log.lock().unwrap()[2].1)
            .unwrap()
            .get("temperature")
            .is_none());
    }

    #[tokio::test]
    async fn stt_uploads_the_wav_as_multipart_with_model_language_and_prompt() {
        let (base, log) = serve(|_, _| (200, r#"{"text":" hello there "}"#.into(), 0));
        let wav = base64::engine::general_purpose::STANDARD.encode(b"RIFF....WAVEfake");
        let r = handle(json!({ "id": 7, "op": "stt", "cands": [cand(&base, "s::m")], "wav_b64": wav, "language": "en", "prompt": "Vocabulary: Kubernetes." })).await;
        assert_eq!((r["ok"].as_bool(), r["text"].as_str()), (Some(true), Some("hello there")), "{r}");
        let (path, body) = log.lock().unwrap()[0].clone();
        assert!(path.ends_with("/audio/transcriptions"));
        let text = String::from_utf8_lossy(&body);
        for needle in ["RIFF....WAVEfake", "name=\"model\"", "name=\"language\"", "Kubernetes", "audio.wav"] {
            assert!(text.contains(needle), "{needle}");
        }
        let auto = handle(json!({ "id": 8, "op": "stt", "cands": [cand(&base, "s::m")], "wav_b64": "QUJD", "language": "auto" })).await;
        assert_eq!(auto["ok"], true);
        assert!(
            !String::from_utf8_lossy(&log.lock().unwrap()[1].1).contains("name=\"language\""),
            "auto language sends no language field"
        );
    }

    #[tokio::test]
    async fn bad_payloads_are_flagged_as_transport_errors() {
        let r = handle(json!({ "id": 9, "op": "stt", "cands": [cand("http://x/v1", "s::m")], "wav_b64": "!!!not base64!!!" })).await;
        assert_eq!((r["ok"].as_bool(), r["transport"].as_bool()), (Some(false), Some(true)));
        assert_eq!(handle(json!({ "id": 10, "op": "nope" })).await["transport"], true);
        assert_eq!(handle(json!({ "id": 11, "op": "hello" })).await["engine"], "murmint-engine");
    }

    #[test]
    fn cloudflare_errors_are_made_actionable() {
        assert!(friendly_cf("Authentication error").contains("API Token"));
        assert!(friendly_cf(
            "Could not route to /client/v4/accounts/x/ai, perhaps your object identifier is invalid?"
        )
        .contains("Account ID"));
        assert_eq!(friendly_cf("Rate limited: slow down"), "Rate limited: slow down");
        assert_eq!(classify(401, r#"{"errors":[{"message":"Authentication error"}]}"#).kind, "auth");
    }

    #[tokio::test]
    async fn ping_checks_the_models_endpoint() {
        let (base, log) = serve(|_, _| (200, r#"{"data":[]}"#.into(), 0));
        let r = handle(json!({ "id": 12, "op": "ping", "cands": [cand(&base, "p::m")] })).await;
        assert_eq!(r["ok"], true);
        assert!(log.lock().unwrap()[0].0.ends_with("/v1/models"));
        let (bad, _) = serve(|_, _| (401, r#"{"error":{"message":"nope"}}"#.into(), 0));
        assert_eq!(
            handle(json!({ "id": 13, "op": "ping", "cands": [cand(&bad, "p::m")] })).await["ok"],
            false
        );
    }
}
