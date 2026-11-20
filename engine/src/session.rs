use crate::core::{self, push};
use crate::formatter;
use crate::hub;
use crate::insertion;
use crate::learn::parse_vocab_command;
use crate::profiles::{resolve_context, Context};
use crate::route;
use crate::rt;
use crate::store::now_ms;
use crate::text::{join, word_count};
use crate::usage::day_key;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

const PCM_RATE: usize = 16_000;
const MAX_PCM_BYTES: usize = PCM_RATE * 2 * 60 * 20;

/// Set by the shell until the person has signed in and finished onboarding; dictation waits for it.
pub static LOCKED: AtomicBool = AtomicBool::new(false);

fn locked() -> bool {
    if LOCKED.load(Ordering::Relaxed) {
        push("gate:needed", "shell", json!({}));
        return true;
    }
    false
}

fn hallucination(s: &str) -> bool {
    crate::re!(r"(?i)^(thanks? you( so much)?( for (watching|listening))?|thanks for watching|you|bye|\.|okay|so)[.!\s]*$").is_match(s.trim()).unwrap_or(false)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum St {
    Idle,
    Listening,
    Paused,
    Processing,
    Success,
    Error,
}
impl St {
    fn name(self) -> &'static str {
        match self {
            St::Idle => "idle",
            St::Listening => "listening",
            St::Paused => "paused",
            St::Processing => "processing",
            St::Success => "success",
            St::Error => "error",
        }
    }
}

struct Rt {
    stream: u64,
    r: String,
    bytes: usize,
    finals: usize,
    stopping: bool,
    failed: bool,
    finished: bool,
    timer: u64,
}
struct Held {
    raw: String,
    sec: f64,
    r: Option<String>,
    cost: f64,
    gap_ms: Option<u64>,
}

struct Sess {
    state: St,
    mode: Option<&'static str>,
    ctx: Option<Context>,
    id: u64,
    inserted: usize,
    session_text: Vec<String>,
    buffer_mode: bool,
    buffer: Vec<Held>,
    rt: Option<Rt>,
    pcm: Vec<u8>,
    next_stream: u64,
    extra_cost: f64,
    aborted: bool,
    rec_launched: bool,
    pending_stop: bool,
    last_error: Option<String>,
    skip_msg: Option<String>,
    notice: Option<String>,
    pending_end: Option<String>,
    piece_gap: Option<u64>,
    unplaced: Vec<String>,
    exe: String,
    stop_watch: u64,
    idle_timer: u64,
    before_task: Option<tokio::task::JoinHandle<Option<String>>>,
    before: String,
}

static SESS: LazyLock<Mutex<Sess>> = LazyLock::new(|| {
    Mutex::new(Sess {
        state: St::Idle,
        mode: None,
        ctx: None,
        id: 0,
        inserted: 0,
        session_text: vec![],
        buffer_mode: false,
        buffer: vec![],
        rt: None,
        pcm: vec![],
        next_stream: 1000,
        extra_cost: 0.0,
        aborted: false,
        rec_launched: false,
        pending_stop: false,
        last_error: None,
        skip_msg: None,
        notice: None,
        pending_end: None,
        piece_gap: None,
        unplaced: vec![],
        exe: String::new(),
        stop_watch: 0,
        idle_timer: 0,
        before_task: None,
        before: String::new(),
    })
});
static INFLIGHT: AtomicUsize = AtomicUsize::new(0);
type Job = Pin<Box<dyn Future<Output = ()> + Send>>;
static QUEUE: Mutex<Option<mpsc::UnboundedSender<Job>>> = Mutex::new(None);
static RT: Mutex<Option<tokio::runtime::Handle>> = Mutex::new(None);
static TIMER_SEQ: AtomicUsize = AtomicUsize::new(1);

fn s() -> std::sync::MutexGuard<'static, Sess> {
    SESS.lock().unwrap()
}
fn cfg() -> Value {
    core::get().store.lock().unwrap().config.clone()
}

pub fn init() {
    *RT.lock().unwrap() = Some(tokio::runtime::Handle::current());
    let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
    *QUEUE.lock().unwrap() = Some(tx);
    tokio::spawn(async move {
        while let Some(job) = rx.recv().await {
            job.await;
            INFLIGHT.fetch_sub(1, Ordering::SeqCst);
        }
    });
}

fn enqueue(fut: impl Future<Output = ()> + Send + 'static) {
    INFLIGHT.fetch_add(1, Ordering::SeqCst);
    if let Some(q) = QUEUE.lock().unwrap().as_ref() {
        let _ = q.send(Box::pin(fut));
    }
}
fn inflight() -> usize {
    INFLIGHT.load(Ordering::SeqCst)
}
fn spawn(fut: impl Future<Output = ()> + Send + 'static) {
    let h = RT.lock().unwrap().clone();
    if let Some(h) = h {
        h.spawn(fut);
    }
}
fn after(ms: u64, f: impl FnOnce() + Send + 'static) {
    spawn(async move {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        f();
    });
}

fn can_start(st: St) -> bool {
    matches!(st, St::Idle | St::Success | St::Error)
}

pub fn summary() -> Value {
    let g = s();
    json!({ "state": g.state.name(), "mode": g.mode })
}
fn is_active(g: &Sess) -> bool {
    matches!(g.state, St::Listening | St::Paused)
}

fn toast(kind: &str, message: &str) {
    push("toast", "main", json!({ "kind": kind, "message": message }));
}
fn partial(_text: &str, _stage: &str) {}
fn recorder(cmd: &str, opts: Value) {
    push("rec:cmd", "overlay", json!({ "cmd": cmd, "opts": opts }));
}

/// Longer pauses with Speech support on.
fn stretch(ms: &Value, support: bool) -> Value {
    match (support, ms.as_u64()) {
        (true, Some(n)) => json!(n * 9 / 5),
        _ => ms.clone(),
    }
}

fn set_state(g: &mut Sess, st: St, extra: Value) {
    g.state = st;
    let mut payload =
        json!({ "state": st.name(), "mode": g.mode, "app": g.ctx.as_ref().map(|c| c.app_name.clone()) });
    if let Some(o) = extra.as_object() {
        for (k, v) in o {
            payload[k] = v.clone();
        }
    }
    push("overlay:state", "overlay", payload.clone());
    push("session:state", "main", payload);
}

pub fn on_hotkey_event(v: Value) {
    let (name, st) =
        (v["name"].as_str().unwrap_or("").to_string(), v["state"].as_str().unwrap_or("").to_string());
    spawn(async move {
        hotkey(&name, &st).await;
    });
}

async fn hotkey(name: &str, st: &str) {
    if st == "down" && name != "cancel" && locked() {
        return;
    }
    let trigger = cfg()["dictation"]["trigger"].as_str().unwrap_or("hold").to_string();
    let (state, mode) = {
        let g = s();
        (g.state, g.mode)
    };
    let active = matches!(state, St::Listening | St::Paused);
    match name {
        "dictate" => {
            if trigger == "hold" {
                if st == "down" {
                    start("hold").await;
                } else if mode == Some("hold") {
                    stop();
                }
            } else if st == "down" {
                if can_start(state) {
                    start("toggle").await;
                } else if active {
                    stop();
                }
            }
        }
        "handsFree" if st == "down" => {
            if can_start(state) {
                start("handsfree").await;
            } else if active {
                stop();
            }
        }
        "cancel" if st == "down" => {
            if state != St::Idle {
                cancel();
            }
        }
        "formatSelection" if st == "down" && state == St::Idle => {
            tokio::spawn(format_selection());
        }
        "undo" if st == "down" => {
            let r = insertion::undo().await;
            if r["ok"] != json!(true) {
                toast("info", r["error"].as_str().unwrap_or("Nothing to undo"));
            }
        }
        _ => {}
    }
}

/// Ctrl+Alt+F: format the selection like a dictation and paste it back.
async fn format_selection() {
    let raw = match crate::insertion::copy_selection().await {
        Some(t) => t,
        None => {
            toast("info", "Select some text first, then press the shortcut");
            return;
        }
    };
    let (app, site) = tokio::task::spawn_blocking(|| {
        let app = crate::native::foreground_full();
        let site = if crate::native::BROWSERS.contains(&app.0.to_lowercase().as_str()) {
            crate::native::site_for(&app.2, &app.1)
        } else {
            None
        };
        (app, site)
    })
    .await
    .unwrap_or_default();
    let ctx = resolve_context(&cfg(), &json!({ "process": app.0, "title": app.1 }));
    // the exe and site give the pill the app's icon, as it has while dictating
    crate::core::push(
        "overlay:state",
        "overlay",
        json!({ "state": "processing", "mode": "toggle", "app": ctx.app_name, "exe": app.2, "site": site }),
    );
    let f = formatter::format(&raw, &ctx, false).await;
    let text = f.text.trim_end().to_string();
    crate::core::push("overlay:state", "overlay", json!({ "state": "idle" }));
    if text.is_empty() || text == raw.trim_end() {
        toast("info", "That text already looks good");
        return;
    }
    let r = insertion::insert(&text).await;
    if !r.ok {
        toast("error", r.error.as_deref().unwrap_or("Couldn't replace the selected text"));
        return;
    }
    let mut st = core::get().store.lock().unwrap();
    if st.config["privacy"]["saveHistory"] != json!(false) {
        st.add_history(json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "ts": now_ms(),
            "app": ctx.app_name,
            "kind": ctx.kind,
            "style": ctx.style["name"],
            "raw": raw,
            "text": text,
            "edited": null,
            "words": word_count(&text),
            "sec": 0,
            "route": f.route,
            "reason": f.reason,
            "fmt": f.r,
            "cost": f.cost,
            "pasted": true,
            "source": "selection",
            "exe": app.2,
            "profile": ctx.profile_id,
        }));
    }
    drop(st);
    core::changed("history");
}

pub async fn action(a: &str) {
    let (state, active) = {
        let g = s();
        (g.state, is_active(&g))
    };
    match a {
        "toggle" => {
            if can_start(state) {
                start("toggle").await;
            } else if active {
                stop();
            }
        }
        "handsfree" => {
            if can_start(state) {
                start("handsfree").await;
            } else if active {
                stop();
            }
        }
        "stop" => stop(),
        "hold" => {
            if can_start(state) {
                start("hold").await;
            }
        }
        "release" => stop(),
        "pause" => pause(),
        "resume" => resume(),
        "cancel" => cancel(),
        _ => {}
    }
}

pub async fn start(mode: &'static str) {
    if locked() {
        return;
    }
    let sid;
    {
        let mut g = s();
        if !can_start(g.state) {
            return;
        }
        g.idle_timer += 1;
        g.id += 1;
        sid = g.id;
        g.mode = Some(mode);
        g.inserted = 0;
        g.session_text.clear();
        g.buffer_mode = false;
        g.buffer.clear();
        g.rt = None;
        g.pcm.clear();
        g.extra_cost = 0.0;
        g.aborted = false;
        g.rec_launched = false;
        g.pending_stop = false;
        g.last_error = None;
        g.skip_msg = None;
        g.notice = None;
        g.pending_end = None;
        g.unplaced.clear();
        g.exe.clear();
    }
    let c = cfg();
    if c["formatting"]["useAI"] != json!(false) {
        let best = {
            let store = core::get().store.lock().unwrap();
            route::rank_chat(&store, 0, 300.0, 100.0).first().cloned()
        };
        if let Some(r) = best {
            hub::prewarm(&r);
        }
    }
    let mut live: Option<(String, Value)> = None;
    if c["dictation"]["realtime"] != json!(false) {
        let lang = crate::accent::language(&c);
        let store = core::get().store.lock().unwrap();
        if let Some(r) = route::rank_realtime(&store, &lang).first().cloned() {
            if let Some(cand) = hub::cands(&store, &[r.clone()], "stt").into_iter().next() {
                live = Some((r, cand));
            }
        }
    }
    if let Some((r, cand)) = live {
        let stream = {
            let mut g = s();
            g.next_stream += 1;
            let n = g.next_stream;
            g.rt = Some(Rt {
                stream: n,
                r: r.clone(),
                bytes: 0,
                finals: 0,
                stopping: false,
                failed: false,
                finished: false,
                timer: 0,
            });
            n
        };
        let keywords: Vec<String> = vec![];
        let language = crate::accent::language(&c);
        let languages = if language == "auto" { vec![] } else { vec![language] };
        let hint = String::new();
        match crate::parse_cands(&json!([cand])).into_iter().next() {
            Some(cand) => rt::open(
                rt::OpenReq { stream, cand, languages, keywords, hint },
                std::sync::Arc::new(on_rt_event),
            ),
            None => rt_fail("Live transcription is unavailable"),
        }
    } else {
        let warm = {
            let store = core::get().store.lock().unwrap();
            route::rank_stt(&store, 5.0).first().cloned()
        };
        if let Some(w) = warm {
            hub::prewarm(&w);
        }
    }

    {
        let mut g = s();
        if g.id != sid {
            return;
        }
        set_state(&mut g, St::Listening, json!({ "starting": true }));
    }
    let (streaming, live_on) =
        (c["dictation"]["streaming"] != json!(false) && mode != "handsfree", s().rt.is_some());
    let d = &c["dictation"];
    let support = d["speechSupport"] == json!(true);
    recorder(
        "start",
        json!({
            "deviceId": d["micDeviceId"], "stream": live_on, "continuous": live_on || mode == "handsfree" || streaming,
            "preview": d["livePreview"] == json!(true) && !streaming && !live_on && mode != "handsfree",
            "silenceMs": if mode == "handsfree" {
                stretch(&d["silenceMs"], support)
            } else {
                json!(d["streamPauseMs"].as_u64().filter(|n| *n > 0).unwrap_or(550))
            },
            "autoStopMs": if mode == "hold" { json!(0) } else { stretch(&json!(d["autoStopMs"].as_u64().unwrap_or(3000)), support) },
            "noiseGate": d["noiseGate"], "sounds": c["appearance"]["sounds"],
        }),
    );
    let (app, site) = tokio::task::spawn_blocking(|| {
        let app = crate::native::foreground_full();
        let site = if crate::native::BROWSERS.contains(&app.0.to_lowercase().as_str()) {
            crate::native::site_for(&app.2, &app.1)
        } else {
            None
        };
        (app, site)
    })
    .await
    .unwrap_or_default();
    let app_v = json!({ "process": app.0, "title": app.1 });
    let ctx = resolve_context(&c, &app_v);
    formatter::warm(&ctx);
    {
        let mut g = s();
        if g.id != sid {
            return;
        }
        g.buffer_mode = ["email", "document", "prompt"].contains(&ctx.kind.as_str());
        g.ctx = Some(ctx.clone());
        g.exe = app.2.clone();
        g.before.clear();
        // read message context now, before anything is typed
        g.before_task = if c["dictation"]["messageContext"] == json!(true) || c["dictation"]["boxJoin"] == json!(true) {
            Some(tokio::task::spawn_blocking(|| crate::native::read_before_cursor(800)))
        } else {
            None
        };
    }
    let stop_now = {
        let mut g = s();
        g.rec_launched = true;
        set_state(
            &mut g,
            St::Listening,
            json!({ "app": ctx.app_name, "kind": ctx.kind, "style": ctx.style["name"], "exe": app.2, "site": site }),
        );
        std::mem::take(&mut g.pending_stop)
    };
    if stop_now {
        stop();
    }
}

pub fn stop() {
    let sid;
    {
        let mut g = s();
        if !is_active(&g) {
            return;
        }
        if !g.rec_launched {
            g.pending_stop = true;
            return;
        }
        set_state(&mut g, St::Processing, json!({}));
        sid = g.id;
    }
    recorder("stop", Value::Null);
    after(12_000, move || {
        let (hang, live_unfinished) = {
            let g = s();
            (
                g.id == sid && g.state == St::Processing && !g.aborted,
                g.rt.as_ref().is_some_and(|r| !r.finished),
            )
        };
        if !hang {
            return;
        }
        if live_unfinished {
            rt_fail("no reply from the live stream");
            return;
        }
        if inflight() == 0 {
            enqueue(async {
                flush().await;
                finalize_result().await;
            });
        }
    });
}

pub fn pause() {
    let g = s();
    if g.state == St::Listening {
        drop(g);
        recorder("pause", Value::Null);
        let mut g = s();
        set_state(&mut g, St::Paused, json!({}));
    }
}
pub fn resume() {
    let g = s();
    if g.state == St::Paused {
        drop(g);
        recorder("resume", Value::Null);
        let mut g = s();
        set_state(&mut g, St::Listening, json!({}));
    }
}

pub fn cancel() {
    {
        let mut g = s();
        if g.state == St::Idle {
            return;
        }
        g.aborted = true;
        g.id += 1;
        g.buffer.clear();
        g.pending_end = None;
    }
    rt_end(true);
    recorder("cancel", Value::Null);
    finish("cancelled", json!({}));
}

fn finish(kind: &str, extra: Value) {
    let mut g = s();
    g.stop_watch += 1;
    if kind == "cancelled" {
        set_state(&mut g, St::Idle, json!({ "result": "cancelled" }));
        g.mode = None;
        return;
    }
    set_state(&mut g, if kind == "success" { St::Success } else { St::Error }, extra);
    g.idle_timer += 1;
    let (timer, hold) = (g.idle_timer, if kind == "success" { 300 } else { 3800 });
    drop(g);
    after(hold, move || {
        let mut g = s();
        if g.idle_timer == timer && inflight() == 0 && !is_active(&g) {
            set_state(&mut g, St::Idle, json!({}));
            g.mode = None;
        }
    });
}

async fn finalize_result() {
    let pending = {
        let mut g = s();
        if g.aborted {
            return;
        }
        g.pending_end.take()
    };
    if pending.is_some() {
        insertion::insert(".").await;
    }
    let mut g = s();
    if g.aborted {
        return;
    }
    let (err, ins, notice, skip) = (g.last_error.take(), g.inserted, g.notice.take(), g.skip_msg.take());
    let text = g.session_text.join(" ");
    let unplaced = std::mem::take(&mut g.unplaced).join(" ");
    drop(g);
    if !unplaced.is_empty() {
        finish("success", json!({ "text": unplaced, "copy": true }));
    } else if let Some(e) = err {
        finish("error", json!({ "message": e }));
    } else if ins > 0 {
        finish("success", json!({ "text": text }));
    } else if let Some(n) = notice {
        finish("success", json!({ "message": n }));
    } else {
        finish("error", json!({ "message": skip.unwrap_or_else(|| "Didn't catch that".into()) }));
    }
}

pub fn rec_error(message: &str) {
    {
        let mut g = s();
        g.aborted = true;
        g.id += 1;
        g.last_error = None;
    }
    finish("error", json!({ "message": message }));
}

pub fn on_chunk(buf: Vec<u8>) {
    let mut g = s();
    if g.aborted {
        return;
    }
    let Some(r) = g.rt.as_mut() else { return };
    r.bytes += buf.len();
    let (stream, failed, keep) = (r.stream, r.failed, r.bytes <= MAX_PCM_BYTES);
    if keep {
        g.pcm.extend_from_slice(&buf);
    }
    drop(g);
    if !failed {
        rt::cmd(stream, rt::Cmd::Audio(buf));
    }
}

pub fn on_auto_stop() {
    let ok = {
        let g = s();
        g.mode != Some("hold") && g.state == St::Listening
    };
    if ok {
        stop();
    }
}

pub fn on_boundary(last: bool, gap_ms: Option<u64>) {
    let mut g = s();
    if g.aborted {
        return;
    }
    let Some(r) = g.rt.as_mut() else { return };
    let (stream, failed) = (r.stream, r.failed);
    let mut fallback = false;
    if last {
        r.stopping = true;
        if failed {
            fallback = true;
        } else {
            g.stop_watch += 1;
        }
    }
    drop(g);
    if !failed {
        rt::cmd(stream, rt::Cmd::Commit { last, gap_ms });
    }
    if fallback {
        rt_fallback();
        return;
    }
    if last {
        let timer = {
            let mut g = s();
            let t = TIMER_SEQ.fetch_add(1, Ordering::SeqCst) as u64;
            if let Some(r) = g.rt.as_mut() {
                r.timer = t;
            }
            t
        };
        after(8000, move || {
            if s().rt.as_ref().is_some_and(|r| r.timer == timer && !r.finished) {
                rt_fail("The live transcription took too long to finish");
            }
        });
    }
}

pub fn on_rt_event(m: Value) {
    let stream = m["stream"].as_u64().unwrap_or(0);
    let ok = {
        let g = s();
        g.rt.as_ref().is_some_and(|r| r.stream == stream) && !g.aborted
    };
    if !ok {
        return;
    }
    match m["type"].as_str().unwrap_or("") {
        "partial" => partial(m["text"].as_str().unwrap_or(""), "live"),
        "final" => {
            if let Some(r) = s().rt.as_mut() {
                r.finals += 1;
            }
            let text = m["text"].as_str().unwrap_or("").to_string();
            partial(&text, "segment");
            let gap = m["gap_ms"].as_u64();
            enqueue(async move {
                rt_piece(text, gap).await;
            });
        }
        "done" => enqueue(async {
            rt_finish().await;
        }),
        "error" => {
            let msg = m["message"].as_str().unwrap_or("").to_string();
            if m["fatal"] == json!(true) {
                rt_fail(&msg);
            } else {
                toast("error", &msg);
            }
        }
        _ => {}
    }
}

async fn rt_piece(text: String, gap: Option<u64>) {
    if s().aborted || text.is_empty() {
        return;
    }
    if hallucination(&text) && word_count(&text) <= 2 {
        return;
    }
    let r = s().rt.as_ref().map(|r| r.r.clone());
    handle_raw(&text, (word_count(&text) as f64 / 2.5).max(1.0), r, 0.0, gap).await;
}

async fn rt_finish() {
    let (r, bytes) = {
        let mut g = s();
        let aborted = g.aborted;
        let Some(rt) = g.rt.as_mut() else { return };
        if rt.finished || aborted {
            return;
        }
        rt.finished = true;
        (rt.r.clone(), rt.bytes)
    };
    let audio_sec = bytes as f64 / (PCM_RATE * 2) as f64;
    let price = {
        let st = core::get().store.lock().unwrap();
        st.find_model(&r).map(|(_, m)| m["price"]["perMinute"].as_f64().unwrap_or(0.0))
    };
    if let (Some(per_min), true) = (price, audio_sec > 0.0) {
        let cost = per_min * audio_sec / 60.0;
        hub::on_usage(json!({ "model": r, "cost": cost, "modelAudioSec": audio_sec }));
        let (hold, ins) = {
            let g = s();
            (!g.buffer.is_empty(), g.inserted)
        };
        let last_id = core::get()
            .store
            .lock()
            .unwrap()
            .history
            .first()
            .map(|h| (h["id"].clone(), h["cost"].as_f64().unwrap_or(0.0)));
        match (hold || ins == 0, last_id) {
            (false, Some((id, old))) => {
                core::get()
                    .store
                    .lock()
                    .unwrap()
                    .update_history(id.as_str().unwrap_or(""), &json!({ "cost": old + cost }));
                core::changed("history");
            }
            _ => s().extra_cost += cost,
        }
    }
    flush().await;
    rt_end(false);
    finalize_result().await;
}

fn rt_fail(message: &str) {
    let stopping = {
        let mut g = s();
        let Some(r) = g.rt.as_mut() else { return };
        if r.finished || r.failed {
            return;
        }
        r.failed = true;
        r.stopping
    };
    toast(
        "info",
        &format!(
            "Live transcription unavailable ({}). Using standard transcription.",
            message.chars().take(120).collect::<String>()
        ),
    );
    if stopping {
        rt_fallback();
    }
}

fn rt_fallback() {
    let (audio, finals) = {
        let mut g = s();
        let Some(r) = g.rt.as_mut() else { return };
        if r.finished {
            return;
        }
        r.finished = true;
        let finals = r.finals;
        (std::mem::take(&mut g.pcm), finals)
    };
    rt_end(true);
    enqueue(async move {
        if finals == 0 && !audio.is_empty() {
            let peak = audio
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs() as f32 / 32768.0)
                .fold(0.0f32, f32::max);
            process(Seg {
                wav: pcm_to_wav(&audio),
                duration_sec: audio.len() as f64 / (PCM_RATE * 2) as f64,
                peak: peak.max(0.05) as f64,
                last: true,
                gap_ms: None,
            })
            .await;
        } else {
            flush().await;
        }
        finalize_result().await;
    });
}

fn rt_end(cancelled: bool) {
    let mut g = s();
    g.pcm.clear();
    if let Some(r) = g.rt.as_ref() {
        if cancelled && !r.failed {
            rt::cmd(r.stream, rt::Cmd::Close);
        }
    }
    if cancelled {
        g.rt = None;
    }
}

fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let mut h: Vec<u8> = Vec::with_capacity(44 + pcm.len());
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&(PCM_RATE as u32).to_le_bytes());
    h.extend_from_slice(&((PCM_RATE * 2) as u32).to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    h.extend_from_slice(pcm);
    h
}

pub struct Seg {
    pub wav: Vec<u8>,
    pub duration_sec: f64,
    pub peak: f64,
    pub last: bool,
    pub gap_ms: Option<u64>,
}

pub fn on_segment(seg: Seg) {
    {
        let mut g = s();
        if g.aborted {
            return;
        }
        if seg.last && g.state != St::Processing {
            set_state(&mut g, St::Processing, json!({}));
        }
    }
    let last = seg.last;
    enqueue(async move {
        process(seg).await;
        if last {
            finalize_result().await;
        }
    });
}

async fn process(seg: Seg) {
    if s().aborted {
        return;
    }
    let c = cfg();
    let d = &c["dictation"];
    let last = seg.last;
    if seg.duration_sec * 1000.0 < d["minMs"].as_f64().unwrap_or(350.0) {
        s().skip_msg = Some("Too short".into());
        if last {
            flush().await;
        }
        return;
    }
    if seg.peak < d["noiseGate"].as_f64().unwrap_or(0.01) {
        s().skip_msg = Some("Didn't hear anything".into());
        if last {
            flush().await;
        }
        return;
    }
    let refs = {
        let st = core::get().store.lock().unwrap();
        route::rank_stt(&st, seg.duration_sec)
    };
    let lang = match d["language"].as_str().unwrap_or("auto") {
        "auto" => crate::accent::language(&c),
        picked => picked.to_string(),
    };
    let prompt = crate::accent::stt_prompt(&c, seg.duration_sec);
    let cleaned = if d["noiseReduction"] != json!(false) {
        let raw = seg.wav.clone();
        tokio::task::spawn_blocking(move || crate::denoise::reduce_noise(&raw)).await.ok()
    } else {
        None
    };
    let stt = match hub::transcribe(
        &refs,
        cleaned.as_deref().unwrap_or(&seg.wav),
        seg.duration_sec,
        if lang == "auto" { None } else { Some(lang.as_str()) },
        &prompt,
    )
    .await
    {
        Ok(mut r) => {
            r.text = crate::accent::strip_echo(&r.text, &prompt);
            r
        }
        Err(e) => {
            s().last_error = Some(if e.not_configured {
                "Add an API key in Models to start dictating".to_string()
            } else {
                e.message.chars().take(160).collect()
            });
            toast("error", &e.message);
            return;
        }
    };
    if s().aborted {
        return;
    }
    if stt.text.is_empty() || (seg.duration_sec < 2.5 && hallucination(&stt.text)) {
        s().skip_msg = Some("Didn't catch that".into());
        if last {
            flush().await;
        }
        return;
    }
    handle_raw(&stt.text, seg.duration_sec, Some(stt.r), stt.cost, seg.gap_ms).await;
    if last {
        flush().await;
    }
}

pub async fn preview(wav: Vec<u8>, duration_sec: f64) -> String {
    {
        let g = s();
        if !is_active(&g) || g.rt.is_some() {
            return String::new();
        }
    }
    let c = cfg();
    let refs = {
        let st = core::get().store.lock().unwrap();
        route::rank_stt(&st, duration_sec).into_iter().take(1).collect::<Vec<_>>()
    };
    let lang = crate::accent::language(&c);
    let prompt = crate::accent::stt_prompt(&c, duration_sec);
    match hub::transcribe(
        &refs,
        &wav,
        duration_sec,
        if lang == "auto" { None } else { Some(lang.as_str()) },
        &prompt,
    )
    .await
    {
        Ok(r) => {
            let text = crate::accent::strip_echo(&r.text, &prompt);
            if hallucination(&text) {
                String::new()
            } else {
                text
            }
        }
        Err(_) => String::new(),
    }
}

async fn handle_raw(raw: &str, sec: f64, r: Option<String>, cost: f64, gap_ms: Option<u64>) {
    let personal = core::get().store.lock().unwrap().config["personal"].clone();
    let raw = crate::learn::strip_vocab_echo(raw, &personal);
    if raw.is_empty() {
        return;
    }
    if let Some(term) = parse_vocab_command(&raw) {
        add_vocab(&term);
        return;
    }

    // Every piece is held, and the whole dictation is formatted once when listening ends.
    let mut g = s();
    if g.ctx.is_some() {
        g.buffer.push(Held { raw, sec, r, cost, gap_ms });
    }
}

fn add_vocab(term: &str) {
    {
        let mut st = core::get().store.lock().unwrap();
        let exists = st.config["personal"]["vocabulary"].as_array().is_some_and(|v| {
            v.iter().any(|e| e["term"].as_str().is_some_and(|t| t.eq_ignore_ascii_case(term)))
        });
        if !exists {
            let entry = json!({ "id": uuid::Uuid::new_v4().to_string(), "term": term, "sounds": [], "note": "voice" });
            if let Some(v) = st.config["personal"]["vocabulary"].as_array_mut() {
                v.push(entry);
            }
            st.touch("config.json");
        }
    }
    core::changed("config");
    let n = format!("Added {term} to your vocabulary");
    s().notice = Some(n.clone());
    toast("ok", &n);
}

async fn flush() {
    let items = {
        let mut g = s();
        if g.buffer.is_empty() || g.aborted {
            return;
        }
        std::mem::take(&mut g.buffer)
    };
    let pieces: Vec<(String, Option<u64>)> = items.iter().map(|i| (i.raw.clone(), i.gap_ms)).collect();
    let raw = join::join_pieces(&pieces);
    let sec: f64 = items.iter().map(|i| i.sec).sum();
    let cost: f64 = items.iter().map(|i| i.cost).sum();
    commit(&raw, sec, items[0].r.clone(), cost, false).await;
}

fn join_prefix(prev: &str, text: &str, gap_ms: Option<u64>) -> (String, String) {
    match join::join_kind(prev, text, gap_ms) {
        join::Join::Period => (". ".into(), text.to_string()),
        join::Join::Comma => (", ".into(), join::after_comma(text)),
        join::Join::Nothing => (" ".into(), join::run_on(text)),
    }
}

async fn message_context() -> String {
    let task = s().before_task.take();
    let Some(task) = task else { return s().before.clone() };
    let text = match tokio::time::timeout(Duration::from_millis(1500), task).await {
        Ok(Ok(Some(t))) => t,
        _ => String::new(),
    };
    s().before = text.clone();
    text
}

async fn commit(raw: &str, sec: f64, stt_ref: Option<String>, stt_cost: f64, local_only: bool) {
    let ctx = {
        let g = s();
        g.ctx.clone()
    };
    let Some(mut ctx) = ctx else { return };
    ctx.before = message_context().await;
    partial(raw, "formatting");
    let f = formatter::format(raw, &ctx, local_only).await;
    if s().aborted {
        return;
    }
    let personal = core::get().store.lock().unwrap().config["personal"].clone();
    let mut text = crate::learn::strip_vocab_echo(&f.text, &personal);
    let keys = insertion::has_keys(&text);
    if text.is_empty() {
        s().skip_msg = Some("Didn't catch that".into());
        return;
    }

    let autotype_on = cfg()["dictation"]["autoType"] != json!(false);
    if !insertion::target_editable().await && !(autotype_on && insertion::autotype().await) {
        let mut g = s();
        if let Some(prev) = g.pending_end.take() {
            if let Some(last) = g.unplaced.last_mut() {
                *last = format!("{prev}.");
            }
        }
        g.unplaced.push(insertion::readable(&text));
        drop(g);
        save_history(raw, &insertion::readable(&text), sec, stt_ref, stt_cost, &f, &ctx, false);
        return;
    }

    let (prefix, withheld);
    let mut erase = 0;
    {
        let mut g = s();
        let mode = g.mode;
        let mut pre = String::new();
        let join = (g.inserted == 0 && g.pending_end.is_none() && cfg()["dictation"]["boxJoin"] == json!(true))
            .then(|| join::continue_in_box(&ctx.before, &text))
            .flatten();
        if let Some((n, joined)) = join {
            erase = n;
            text = joined;
        } else if let Some(prev) = g.pending_end.take() {
            let (p, t) = join_prefix(&prev, &text, g.piece_gap);
            pre = p;
            text = t;
        } else if g.inserted > 0
            && !text.starts_with(char::is_whitespace)
            && !g.session_text.last().is_some_and(|t| t.ends_with('\n'))
        {
            pre = " ".into();
        }
        let can_hold = local_only
            && !keys
            && mode != Some("handsfree")
            && matches!(g.state, St::Listening | St::Paused)
            && text.ends_with('.')
            && !text.ends_with("..")
            && !text.contains('\n');
        withheld = if can_hold {
            text.pop();
            Some(text.clone())
        } else {
            None
        };
        g.pending_end = withheld.clone();
        prefix = pre;
    }
    if erase > 0 {
        insertion::erase(erase).await;
    }
    let res = insertion::deliver(&format!("{prefix}{text}")).await;
    if !res.ok {
        s().last_error = None;
        toast("info", "Couldn’t paste automatically, the text is on your clipboard. Press Ctrl+V.");
    }
    {
        let mut g = s();
        g.inserted += 1;
        g.session_text.push(insertion::readable(&text));
    }
    save_history(raw, &insertion::readable(&text), sec, stt_ref, stt_cost, &f, &ctx, res.ok);
}

fn save_history(
    raw: &str,
    text: &str,
    sec: f64,
    stt_ref: Option<String>,
    stt_cost: f64,
    f: &formatter::FormatOut,
    ctx: &Context,
    pasted: bool,
) {
    let words = word_count(text);
    let (extra, ctx_app, exe) = {
        let mut g = s();
        (std::mem::take(&mut g.extra_cost), g.ctx.clone(), g.exe.clone())
    };
    let cost = stt_cost + extra + f.cost;
    let save = core::get().store.lock().unwrap().config["privacy"]["saveHistory"] != json!(false);
    {
        let mut st = core::get().store.lock().unwrap();
        if save {
            let app = ctx_app.as_ref().map(|c| c.app_name.clone()).unwrap_or_default();
            st.add_history(json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "ts": now_ms(),
                "app": app,
                "exe": exe,
                "profile": ctx_app.as_ref().and_then(|c| c.profile_id.clone()),
                "kind": ctx.kind,
                "style": ctx.style["name"],
                "raw": raw,
                "text": text,
                "edited": null,
                "words": words,
                "sec": (sec * 10.0).round() / 10.0,
                "route": f.route,
                "reason": f.reason,
                "stt": stt_ref,
                "fmt": f.r,
                "cost": cost,
                "pasted": pasted,
            }));
        }
        st.record_usage(&day_key(now_ms()), &json!({ "dictations": 1, "words": words, "audioSec": sec }));
    }
    core::changed("history");
    partial(text, "done");
}

#[cfg(all(test, local_tests))]
pub fn test_reset() {
    let mut g = s();
    g.unplaced.clear();
    g.state = St::Idle;
    g.mode = None;
    g.pending_end = None;
    g.aborted = false;
    g.buffer.clear();
    g.inserted = 0;
    g.session_text.clear();
    g.rt = None;
    g.pcm.clear();
    g.last_error = None;
    g.skip_msg = None;
    g.notice = None;
    g.extra_cost = 0.0;
    INFLIGHT.store(0, Ordering::SeqCst);
}
#[cfg(all(test, local_tests))]
pub fn test_inflight() -> usize {
    inflight()
}
#[cfg(all(test, local_tests))]
pub fn test_done() -> bool {
    matches!(s().state, St::Success | St::Error | St::Idle)
}
#[cfg(all(test, local_tests))]
pub fn test_state() -> &'static str {
    s().state.name()
}
#[cfg(all(test, local_tests))]
pub fn test_show_error() {
    let mut g = s();
    set_state(&mut g, St::Error, json!({ "message": "Didn't catch that" }));
}
