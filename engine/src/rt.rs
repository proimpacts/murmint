use crate::Cand;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

pub type Sink = Arc<dyn Fn(Value) + Send + Sync>;

pub enum Cmd {
    Audio(Vec<u8>),
    Commit { last: bool, gap_ms: Option<u64> },
    Close,
}

pub struct OpenReq {
    pub stream: u64,
    pub cand: Cand,
    pub languages: Vec<String>,
    pub keywords: Vec<String>,
    pub hint: String,
}

struct Attach {
    stream: u64,
    cmd_rx: mpsc::UnboundedReceiver<Cmd>,
    languages: Vec<String>,
    keywords: Vec<String>,
    hint: String,
    sink: Sink,
}

struct Slot {
    attach: mpsc::Sender<Attach>,
    busy: Arc<AtomicBool>,
}

static STREAMS: LazyLock<Mutex<HashMap<u64, mpsc::UnboundedSender<Cmd>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SLOTS: LazyLock<Mutex<HashMap<String, Slot>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

const IDLE_KEEP: Duration = Duration::from_secs(50);
const FINAL_WAIT: Duration = Duration::from_secs(4);
const MIN_COMMIT_SAMPLES: usize = 24_000 * 12 / 100;

pub fn ws_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    let ws = if let Some(r) = b.strip_prefix("https://") {
        format!("wss://{r}")
    } else if let Some(r) = b.strip_prefix("http://") {
        format!("ws://{r}")
    } else {
        b.to_string()
    };
    format!("{ws}/realtime?intent=transcription")
}

fn slot_key(c: &Cand) -> String {
    let h = c.key.bytes().fold(14695981039346656037u64, |a, b| (a ^ b as u64).wrapping_mul(1099511628211));
    format!("{}|{}|{h:x}", c.base_url, c.model)
}

pub fn resample_16k_to_24k(pcm: &[u8]) -> Vec<u8> {
    let s: Vec<i16> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    if s.is_empty() {
        return vec![];
    }
    let out_len = s.len() * 3 / 2;
    let mut out = Vec::with_capacity(out_len * 2);
    for j in 0..out_len {
        let pos = j as f64 / 1.5;
        let i = pos.floor() as usize;
        let frac = pos - i as f64;
        let a = s[i.min(s.len() - 1)] as f64;
        let b = s[(i + 1).min(s.len() - 1)] as f64;
        out.extend_from_slice(&((a + (b - a) * frac).round() as i16).to_le_bytes());
    }
    out
}

fn session_update(c: &Cand, languages: &[String], keywords: &[String], hint: &str) -> Value {
    let live = c.model.contains("live");
    let mut tr = json!({ "model": c.model });
    if live {
        if !languages.is_empty() {
            tr["languages"] = json!(languages);
        }
        if !keywords.is_empty() {
            tr["keywords"] = json!(keywords);
        }
    } else {
        if let Some(l) = languages.first() {
            tr["language"] = json!(l);
        }
        let vocab =
            if keywords.is_empty() { String::new() } else { format!("Vocabulary: {}.", keywords.join(", ")) };
        let prompt =
            [hint, vocab.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" ");
        if !prompt.is_empty() {
            tr["prompt"] = json!(prompt);
        }
    }
    json!({ "type": "session.update", "session": { "type": "transcription", "audio": { "input": {
        "format": { "type": "audio/pcm", "rate": 24000 }, "transcription": tr, "turn_detection": null } } } })
}

fn emit(sink: &Sink, stream: u64, mut v: Value) {
    v["ev"] = json!("rt");
    v["stream"] = json!(stream);
    sink(v);
}

pub fn open(req: OpenReq, sink: Sink) {
    let (tx, rx) = mpsc::unbounded_channel();
    STREAMS.lock().unwrap().insert(req.stream, tx);
    let attach = Attach {
        stream: req.stream,
        cmd_rx: rx,
        languages: req.languages,
        keywords: req.keywords,
        hint: req.hint,
        sink,
    };
    let key = slot_key(&req.cand);
    let attach = {
        let slots = SLOTS.lock().unwrap();
        match slots.get(&key) {
            Some(slot) if !slot.busy.swap(true, Ordering::SeqCst) => match slot.attach.try_send(attach) {
                Ok(()) => return,
                Err(e) => {
                    slot.busy.store(false, Ordering::SeqCst);
                    e.into_inner()
                }
            },
            _ => attach,
        }
    };
    tokio::spawn(task(key, req.cand, attach));
}

pub fn cmd(stream: u64, c: Cmd) {
    if let Some(tx) = STREAMS.lock().unwrap().get(&stream) {
        let _ = tx.send(c);
    }
}

async fn task(key: String, cand: Cand, first: Attach) {
    let (att_tx, att_rx) = mpsc::channel::<Attach>(1);
    let busy = Arc::new(AtomicBool::new(true));
    SLOTS.lock().unwrap().insert(key.clone(), Slot { attach: att_tx, busy: busy.clone() });
    run(&cand, first, att_rx, busy.clone()).await;
    let mut s = SLOTS.lock().unwrap();
    if s.get(&key).map(|x| Arc::ptr_eq(&x.busy, &busy)).unwrap_or(false) {
        s.remove(&key);
    }
}

#[derive(Default)]
struct State {
    partial: HashMap<String, String>,
    queue: VecDeque<String>,
    completed: HashMap<String, Option<String>>,
    pending: usize,
    gaps: VecDeque<Option<u64>>,
    item_gap: HashMap<String, Option<u64>>,
    since_commit: usize,
    total_samples: u64,
    finalizing: bool,
}

struct Cur {
    att: Attach,
    st: State,
}

fn flush_ready(cur: &mut Cur) {
    while let Some(front) = cur.st.queue.front().cloned() {
        match cur.st.completed.remove(&front) {
            Some(Some(text)) => {
                emit(
                    &cur.att.sink,
                    cur.att.stream,
                    json!({
                        "type": "final",
                        "text": text,
                        "item": front,
                        "gap_ms": cur.st.item_gap.remove(&front).flatten(),
                    }),
                );
                cur.st.queue.pop_front();
            }
            Some(None) => {
                cur.st.queue.pop_front();
            }
            None => break,
        }
    }
}

fn on_server_event(cur: &mut Cur, v: &Value) {
    let ty = v["type"].as_str().unwrap_or("");
    let item = v["item_id"].as_str().unwrap_or("").to_string();
    match ty {
        "conversation.item.input_audio_transcription.delta" => {
            let acc = cur.st.partial.entry(item.clone()).or_default();
            acc.push_str(v["delta"].as_str().unwrap_or(""));
            emit(
                &cur.att.sink,
                cur.att.stream,
                json!({ "type": "partial", "text": acc.clone(), "item": item }),
            );
        }
        "conversation.item.input_audio_transcription.completed" => {
            cur.st.partial.remove(&item);
            if !cur.st.queue.contains(&item) {
                cur.st.queue.push_back(item.clone());
            }
            cur.st.completed.insert(item, Some(v["transcript"].as_str().unwrap_or("").trim().to_string()));
            cur.st.pending = cur.st.pending.saturating_sub(1);
            flush_ready(cur);
        }
        "conversation.item.input_audio_transcription.failed" => {
            cur.st.partial.remove(&item);
            if !cur.st.queue.contains(&item) {
                cur.st.queue.push_back(item.clone());
            }
            cur.st.completed.insert(item, None);
            cur.st.pending = cur.st.pending.saturating_sub(1);
            flush_ready(cur);
        }
        "input_audio_buffer.committed" => {
            let gap = cur.st.gaps.pop_front().flatten();
            if !item.is_empty() {
                cur.st.item_gap.insert(item.clone(), gap);
            }
            if !item.is_empty() && !cur.st.queue.contains(&item) {
                cur.st.queue.push_back(item);
            }
        }
        "error" => {
            let msg = v["error"]["message"].as_str().unwrap_or("provider error").to_string();
            let code = v["error"]["code"].as_str().unwrap_or("").to_string();
            let empty_commit = code.contains("commit_empty")
                || msg.to_lowercase().contains("buffer too small")
                || msg.to_lowercase().contains("empty");
            if empty_commit {
                cur.st.pending = cur.st.pending.saturating_sub(1);
                cur.st.gaps.pop_front();
            } else {
                emit(
                    &cur.att.sink,
                    cur.att.stream,
                    json!({ "type": "error", "message": msg, "fatal": false }),
                );
            }
        }
        _ => {}
    }
}

fn finished(cur: &Cur) -> bool {
    cur.st.finalizing && cur.st.pending == 0 && cur.st.queue.is_empty()
}

async fn run(cand: &Cand, first: Attach, mut att_rx: mpsc::Receiver<Attach>, busy: Arc<AtomicBool>) {
    let url = ws_url(&cand.base_url);
    let fail = |a: &Attach, message: String| {
        emit(&a.sink, a.stream, json!({ "type": "error", "message": message, "fatal": true }));
        STREAMS.lock().unwrap().remove(&a.stream);
    };
    let mut request = match url.as_str().into_client_request() {
        Ok(r) => r,
        Err(e) => return fail(&first, format!("bad real-time address: {e}")),
    };
    if !cand.key.is_empty() {
        request.headers_mut().insert("authorization", format!("Bearer {}", cand.key).parse().unwrap());
    }
    let ws = match tokio::time::timeout(Duration::from_secs(10), connect_async(request)).await {
        Ok(Ok((ws, _))) => ws,
        Ok(Err(e)) => return fail(&first, format!("Couldn't open the live connection: {e}")),
        Err(_) => return fail(&first, "Live connection timed out".into()),
    };
    let (mut tx, mut rx) = ws.split();

    let mut cur = Some(Cur { att: first, st: State::default() });
    if !configure(&mut tx, cand, cur.as_ref().unwrap()).await {
        return;
    }
    let mut idle_deadline = Instant::now() + IDLE_KEEP;
    let mut final_deadline: Option<Instant> = None;
    let mut ping = tokio::time::interval(Duration::from_secs(20));
    ping.tick().await;

    macro_rules! detach {
        ($ev:expr) => {{
            if let Some(c) = cur.take() {
                emit(&c.att.sink, c.att.stream, json!({ "type": $ev }));
                STREAMS.lock().unwrap().remove(&c.att.stream);
            }
            final_deadline = None;
            idle_deadline = Instant::now() + IDLE_KEEP;
            busy.store(false, Ordering::SeqCst);
        }};
    }

    loop {
        let idle = cur.is_none();
        let cmd = async {
            match cur.as_mut() {
                Some(c) => c.att.cmd_rx.recv().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let (Some(c), Ok(v)) = (cur.as_mut(), serde_json::from_str::<Value>(&t)) {
                            on_server_event(c, &v);
                            if finished(c) { detach!("done"); }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        if let Some(c) = cur.as_ref() {
                            emit(&c.att.sink, c.att.stream, json!({ "type": "error", "message": "The live connection closed", "fatal": true }));
                            STREAMS.lock().unwrap().remove(&c.att.stream);
                        }
                        break;
                    }
                    _ => {}
                }
            }
            c = cmd => {
                match c {
                    Some(Cmd::Audio(pcm)) => {
                        if let Some(cu) = cur.as_mut() {
                            let up = resample_16k_to_24k(&pcm);
                            let n = up.len() / 2;
                            cu.st.since_commit += n;
                            cu.st.total_samples += n as u64;
                            let ev = json!({ "type": "input_audio_buffer.append", "audio": base64::engine::general_purpose::STANDARD.encode(&up) });
                            if tx.send(Message::text(ev.to_string())).await.is_err() { break; }
                        }
                    }
                    Some(Cmd::Commit { last, gap_ms }) => {
                        if let Some(cu) = cur.as_mut() {
                            if cu.st.since_commit >= MIN_COMMIT_SAMPLES {
                                let commit = json!({ "type": "input_audio_buffer.commit" }).to_string();
                                if tx.send(Message::text(commit)).await.is_err() {
                                    break;
                                }
                                cu.st.pending += 1;
                                cu.st.gaps.push_back(gap_ms);
                                cu.st.since_commit = 0;
                            }
                            if last {
                                cu.st.finalizing = true;
                                final_deadline = Some(Instant::now() + FINAL_WAIT);
                            }
                            if finished(cu) { detach!("done"); }
                        }
                    }
                    Some(Cmd::Close) | None => {
                        if cur.is_some() {
                            let _ = tx.send(Message::text(json!({ "type": "input_audio_buffer.clear" }).to_string())).await;
                            detach!("closed");
                        }
                    }
                }
            }
            a = att_rx.recv(), if idle => {
                match a {
                    Some(att) => {
                        cur = Some(Cur { att, st: State::default() });
                        if !configure(&mut tx, cand, cur.as_ref().unwrap()).await { break; }
                    }
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(final_deadline.unwrap_or(idle_deadline)), if final_deadline.is_some() => {
                detach!("done");
            }
            _ = tokio::time::sleep_until(idle_deadline), if idle => { break; }
            _ = ping.tick() => { if tx.send(Message::Ping(vec![].into())).await.is_err() { break; } }
        }
    }
    if let Some(c) = cur.take() {
        STREAMS.lock().unwrap().remove(&c.att.stream);
    }
    busy.store(true, Ordering::SeqCst);
    let _ = tx.close().await;
}

async fn configure<S>(tx: &mut S, cand: &Cand, cur: &Cur) -> bool
where
    S: futures_util::Sink<Message> + Unpin,
{
    let su = session_update(cand, &cur.att.languages, &cur.att.keywords, &cur.att.hint);
    if tx.send(Message::text(su.to_string())).await.is_err() {
        emit(
            &cur.att.sink,
            cur.att.stream,
            json!({ "type": "error", "message": "The live connection closed", "fatal": true }),
        );
        STREAMS.lock().unwrap().remove(&cur.att.stream);
        return false;
    }
    emit(&cur.att.sink, cur.att.stream, json!({ "type": "ready" }));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_hdr_async;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    #[derive(Default)]
    struct Mock {
        connections: usize,
        auth: Vec<String>,
        updates: Vec<Value>,
        append_bytes: Vec<usize>,
        commits: usize,
        paths: Vec<String>,
    }

    async fn mock(out_of_order: bool) -> (String, Arc<Mutex<Mock>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}/v1", l.local_addr().unwrap().port());
        let stats = Arc::new(Mutex::new(Mock::default()));
        let st = stats.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = l.accept().await {
                let st = st.clone();
                tokio::spawn(async move {
                    let st2 = st.clone();
                    let cb = move |req: &Request, resp: Response| {
                        let mut s = st2.lock().unwrap();
                        s.connections += 1;
                        s.auth.push(
                            req.headers()
                                .get("authorization")
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("")
                                .to_string(),
                        );
                        s.paths.push(req.uri().to_string());
                        Ok(resp)
                    };
                    let Ok(ws) = accept_hdr_async(sock, cb).await else { return };
                    let (mut w, mut r) = ws.split();
                    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
                    tokio::spawn(async move {
                        while let Some(m) = out_rx.recv().await {
                            if w.send(Message::text(m)).await.is_err() {
                                break;
                            }
                        }
                    });
                    let mut appends = 0;
                    while let Some(Ok(Message::Text(t))) = r.next().await {
                        let v: Value = serde_json::from_str(&t).unwrap();
                        match v["type"].as_str().unwrap_or("") {
                            "session.update" => st.lock().unwrap().updates.push(v),
                            "input_audio_buffer.append" => {
                                let n = base64::engine::general_purpose::STANDARD
                                    .decode(v["audio"].as_str().unwrap())
                                    .unwrap()
                                    .len();
                                st.lock().unwrap().append_bytes.push(n);
                                appends += 1;
                                if appends % 2 == 0 {
                                    let _ = out_tx.send(json!({ "type": "conversation.item.input_audio_transcription.delta", "item_id": "live", "delta": if appends == 2 { "hello" } else { " there" } }).to_string());
                                }
                            }
                            "input_audio_buffer.commit" => {
                                let k = {
                                    let mut s = st.lock().unwrap();
                                    s.commits += 1;
                                    s.commits
                                };
                                let item = format!("item_{k}");
                                let _ = out_tx.send(
                                    json!({ "type": "input_audio_buffer.committed", "item_id": item })
                                        .to_string(),
                                );
                                let delay = if out_of_order {
                                    if k == 1 {
                                        250
                                    } else {
                                        40
                                    }
                                } else {
                                    30
                                };
                                let tx2 = out_tx.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_millis(delay)).await;
                                    let _ = tx2.send(json!({ "type": "conversation.item.input_audio_transcription.completed", "item_id": item, "transcript": format!("segment {k}") }).to_string());
                                });
                            }
                            _ => {}
                        }
                    }
                });
            }
        });
        (base, stats)
    }

    fn sink() -> (Sink, mpsc::UnboundedReceiver<Value>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(move |v| {
                let _ = tx.send(v);
            }),
            rx,
        )
    }
    fn cand(base: &str, model: &str) -> Cand {
        Cand {
            reference: "p::m".into(),
            ptype: "openai".into(),
            base_url: base.into(),
            account_id: String::new(),
            model: model.into(),
            key: "sk-test".into(),
        }
    }
    fn pcm(samples: usize, v: i16) -> Vec<u8> {
        (0..samples).flat_map(|_| v.to_le_bytes()).collect()
    }
    async fn next(rx: &mut mpsc::UnboundedReceiver<Value>, ty: &str) -> Value {
        loop {
            let v = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("timed out waiting for an event")
                .expect("sink closed");
            if v["type"] == ty {
                return v;
            }
            assert_ne!(v["type"], "error", "unexpected error: {v}");
        }
    }

    #[test]
    fn urls_and_resampling() {
        assert_eq!(
            ws_url("https://api.openai.com/v1"),
            "wss://api.openai.com/v1/realtime?intent=transcription"
        );
        assert_eq!(ws_url("http://127.0.0.1:9/v1/"), "ws://127.0.0.1:9/v1/realtime?intent=transcription");
        assert_eq!(resample_16k_to_24k(&pcm(1024, 100)).len(), 1536 * 2);
        assert!(resample_16k_to_24k(&[]).is_empty());
        let out = resample_16k_to_24k(&pcm(10, 1000));
        assert!(
            out.chunks(2).all(|b| i16::from_le_bytes([b[0], b[1]]) == 1000),
            "a constant signal stays constant"
        );
    }

    #[tokio::test]
    async fn session_is_configured_and_audio_is_resampled_and_streamed() {
        let (base, stats) = mock(false).await;
        let (s, mut rx) = sink();
        open(
            OpenReq {
                stream: 9001,
                cand: cand(&base, "gpt-live-transcribe"),
                languages: vec!["en".into()],
                keywords: vec!["Kubernetes".into(), "Siobhan".into()],
                hint: String::new(),
            },
            s,
        );
        next(&mut rx, "ready").await;
        cmd(9001, Cmd::Audio(pcm(1024, 500)));
        cmd(9001, Cmd::Audio(pcm(1024, 500)));
        let p = next(&mut rx, "partial").await;
        assert_eq!(p["text"], "hello");
        cmd(9001, Cmd::Close);
        next(&mut rx, "closed").await;
        let s = stats.lock().unwrap();
        assert_eq!(s.auth[0], "Bearer sk-test");
        assert!(s.paths[0].contains("/v1/realtime?intent=transcription"), "{}", s.paths[0]);
        let u = &s.updates[0];
        assert_eq!(u["type"], "session.update");
        assert_eq!(u["session"]["type"], "transcription");
        assert_eq!(u["session"]["audio"]["input"]["format"], json!({ "type": "audio/pcm", "rate": 24000 }));
        assert_eq!(u["session"]["audio"]["input"]["transcription"]["model"], "gpt-live-transcribe");
        assert_eq!(u["session"]["audio"]["input"]["transcription"]["languages"], json!(["en"]));
        assert_eq!(
            u["session"]["audio"]["input"]["transcription"]["keywords"],
            json!(["Kubernetes", "Siobhan"])
        );
        assert!(u["session"]["audio"]["input"]["turn_detection"].is_null());
        assert_eq!(s.append_bytes, vec![1536 * 2, 1536 * 2], "16 kHz in, 24 kHz out");
    }

    #[test]
    fn an_accent_hint_goes_in_front_of_the_vocabulary_for_older_models_only() {
        let older = cand("http://x/v1", "gpt-4o-transcribe");
        let u = session_update(
            &older,
            &["en".into()],
            &["Siobhan".into()],
            "The speaker has an Indian accent and is speaking English.",
        );
        assert_eq!(
            u["session"]["audio"]["input"]["transcription"]["prompt"],
            "The speaker has an Indian accent and is speaking English. Vocabulary: Siobhan."
        );
        let only_hint =
            session_update(&older, &[], &[], "The speaker has a British accent and is speaking English.");
        assert_eq!(
            only_hint["session"]["audio"]["input"]["transcription"]["prompt"],
            "The speaker has a British accent and is speaking English."
        );
        assert!(session_update(&older, &[], &[], "")["session"]["audio"]["input"]["transcription"]
            .get("prompt")
            .is_none());
    }

    #[tokio::test]
    async fn older_models_get_language_and_prompt_instead_of_structured_hints() {
        let c = cand("http://x/v1", "gpt-4o-mini-transcribe");
        let u = session_update(&c, &["fr".into()], &["Siobhan".into()], "");
        let t = &u["session"]["audio"]["input"]["transcription"];
        assert_eq!(
            (t["language"].as_str(), t["prompt"].as_str()),
            (Some("fr"), Some("Vocabulary: Siobhan."))
        );
        assert!(t.get("languages").is_none() && t.get("keywords").is_none());
    }

    #[tokio::test]
    async fn segments_arrive_in_spoken_order_even_when_the_provider_finishes_them_out_of_order() {
        let (base, _) = mock(true).await;
        let (s, mut rx) = sink();
        open(
            OpenReq {
                stream: 9002,
                cand: cand(&base, "gpt-live-transcribe"),
                languages: vec![],
                keywords: vec![],
                hint: String::new(),
            },
            s,
        );
        next(&mut rx, "ready").await;
        cmd(9002, Cmd::Audio(pcm(4096, 300)));
        cmd(9002, Cmd::Commit { last: false, gap_ms: None });
        cmd(9002, Cmd::Audio(pcm(4096, 300)));
        cmd(9002, Cmd::Commit { last: true, gap_ms: None });
        let first = next(&mut rx, "final").await;
        let second = next(&mut rx, "final").await;
        assert_eq!((first["text"].as_str(), second["text"].as_str()), (Some("segment 1"), Some("segment 2")));
        next(&mut rx, "done").await;
    }

    #[tokio::test]
    async fn tiny_commits_are_skipped_and_an_empty_dictation_finishes_immediately() {
        let (base, stats) = mock(false).await;
        let (s, mut rx) = sink();
        open(
            OpenReq {
                stream: 9003,
                cand: cand(&base, "gpt-live-transcribe"),
                languages: vec![],
                keywords: vec![],
                hint: String::new(),
            },
            s,
        );
        next(&mut rx, "ready").await;
        cmd(9003, Cmd::Audio(pcm(160, 100)));
        cmd(9003, Cmd::Commit { last: true, gap_ms: None });
        next(&mut rx, "done").await;
        assert_eq!(stats.lock().unwrap().commits, 0, "no commit was sent for 10 ms of audio");
    }

    #[tokio::test]
    async fn the_connection_stays_open_between_dictations() {
        let (base, stats) = mock(false).await;
        for (i, stream) in [9004u64, 9005].iter().enumerate() {
            let (s, mut rx) = sink();
            open(
                OpenReq {
                    stream: *stream,
                    cand: cand(&base, "gpt-live-transcribe"),
                    languages: vec![],
                    keywords: vec![],
                    hint: String::new(),
                },
                s,
            );
            next(&mut rx, "ready").await;
            cmd(*stream, Cmd::Audio(pcm(4096, 300)));
            cmd(*stream, Cmd::Commit { last: true, gap_ms: None });
            let f = next(&mut rx, "final").await;
            assert_eq!(f["text"], format!("segment {}", i + 1));
            next(&mut rx, "done").await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let s = stats.lock().unwrap();
        assert_eq!(s.connections, 1, "the second dictation reused the first connection");
        assert_eq!(s.updates.len(), 2, "but the session is reconfigured for each one");
    }

    #[tokio::test]
    async fn a_failed_connection_is_reported_as_a_fatal_error() {
        let (s, mut rx) = sink();
        open(
            OpenReq {
                stream: 9006,
                cand: cand("http://127.0.0.1:1/v1", "gpt-live-transcribe"),
                languages: vec![],
                keywords: vec![],
                hint: String::new(),
            },
            s,
        );
        let e = tokio::time::timeout(Duration::from_secs(12), rx.recv()).await.unwrap().unwrap();
        assert_eq!((e["type"].as_str(), e["fatal"].as_bool()), (Some("error"), Some(true)));
        assert!(e["message"].as_str().unwrap().contains("live connection"));
    }
}
