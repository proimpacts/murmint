//! Murmint 1, the speech model everyone gets: OpenAI's Whisper Tiny (MIT license) run by whisper.cpp on this computer,
//! so speech to text costs nothing. Formatting goes to the providers the person adds.
//! The model and the server are pinned to one version and checked against their SHA-256 before they are used.
//!
//! The server listens on 127.0.0.1 only, behind a small proxy that wants a key made fresh each launch, so other
//! programs and web pages on this computer can't use it. The rest of the engine sees one OpenAI-style provider.

use crate::core;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const PROVIDER: &str = "murmint-local";
const STT_MODEL: &str = "whisper-tiny";
const MODEL_FILE: &str = "ggml-tiny-q5_1.bin";
const SPARE: u64 = 50 * 1024 * 1024; // room for the zip while it unpacks

struct Part {
    id: &'static str,
    label: &'static str,
    url: &'static str,
    size: u64,
    sha256: &'static str,
    zip: bool,
}

const PARTS: [Part; 2] = [
    Part {
        id: "whisper",
        label: "Speech engine",
        url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip",
        size: 8928640,
        sha256: "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb",
        zip: true,
    },
    Part {
        id: "stt",
        label: "Murmint 1",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny-q5_1.bin",
        size: 32152673,
        sha256: "818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7",
        zip: false,
    },
];
const TOTAL: u64 = PARTS[0].size + PARTS[1].size;

// the server and the CPU builds of ggml it picks from at run time
fn whisper_files() -> Vec<String> {
    let cpu = ["alderlake", "cannonlake", "cascadelake", "haswell", "icelake", "sandybridge", "skylakex", "sse42", "x64"];
    ["whisper-server.exe", "whisper.dll", "ggml.dll", "ggml-base.dll"]
        .iter()
        .map(|f| f.to_string())
        .chain(cpu.iter().map(|n| format!("ggml-cpu-{n}.dll")))
        .map(|f| format!("Release/{f}"))
        .collect()
}

fn root() -> PathBuf {
    let dir = core::get().store.lock().unwrap().dir.clone();
    dir.parent().unwrap_or(&dir).join("models")
}
fn done(p: &Part) -> bool {
    std::fs::read_to_string(root().join(format!("{}.ok", p.id))).is_ok_and(|s| s == p.sha256)
}
pub fn installed() -> bool {
    PARTS.iter().all(done)
}

#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let _ = std::fs::create_dir_all(dir);
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut avail = 0u64;
    unsafe { GetDiskFreeSpaceExW(windows::core::PCWSTR(wide.as_ptr()), Some(&mut avail), None, None).ok()? };
    Some(avail)
}
#[cfg(not(windows))]
fn free_space(_: &Path) -> Option<u64> {
    None
}

// ---- download ----

#[derive(Clone)]
struct Progress {
    state: &'static str,
    received: u64,
    error: Option<String>,
    checking: bool,
}
static PROGRESS: Mutex<Progress> = Mutex::new(Progress { state: "none", received: 0, error: None, checking: false });
static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn status_of(p: &Progress) -> Value {
    let installed = installed();
    let state = if DOWNLOADING.load(Ordering::SeqCst) { "downloading" } else if installed { "installed" } else { p.state };
    json!({ "state": state, "received": p.received, "total": TOTAL, "error": p.error, "checking": p.checking,
            "installed": installed, "running": running(), "size": TOTAL })
}

pub fn status() -> Value {
    let mut s = status_of(&PROGRESS.lock().unwrap());
    s["free"] = json!(free_space(&root()));
    s
}

fn report(change: impl FnOnce(&mut Progress)) {
    let p = {
        let mut g = PROGRESS.lock().unwrap();
        change(&mut g);
        g.clone()
    };
    core::push("local:progress", "main", status_of(&p));
}

fn gb(n: u64) -> String {
    format!("{:.1} GB", n as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Downloads Murmint 1 in the background. Once it is in place, a signed-in, set-up Murmint starts using it.
pub fn download() -> Value {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return status();
    }
    if installed() {
        DOWNLOADING.store(false, Ordering::SeqCst);
        tokio::spawn(after_download());
        return status();
    }
    let left: u64 = PARTS.iter().filter(|p| !done(p)).map(|p| p.size).sum();
    if let Some(free) = free_space(&root()) {
        if free < left + SPARE {
            DOWNLOADING.store(false, Ordering::SeqCst);
            let msg = format!("Not enough disk space. Murmint 1 needs {} free and this drive has {}.", gb(left + SPARE), gb(free));
            report(|p| {
                p.state = "error";
                p.error = Some(msg);
            });
            return status();
        }
    }
    CANCEL.store(false, Ordering::SeqCst);
    report(|p| {
        p.state = "downloading";
        p.error = None;
        p.received = 0;
    });
    tokio::spawn(async {
        let result = async {
            let mut base = 0;
            for p in &PARTS {
                if !done(p) {
                    install(p, base).await?;
                }
                base += p.size;
                report(|g| g.received = base);
            }
            Ok::<(), String>(())
        }
        .await;
        DOWNLOADING.store(false, Ordering::SeqCst);
        match result {
            Ok(()) => {
                report(|p| p.state = "installed");
                after_download().await;
            }
            Err(e) => {
                let cancelled = CANCEL.load(Ordering::SeqCst);
                report(|p| {
                    p.state = if cancelled { "none" } else { "error" };
                    p.error = (!cancelled).then_some(e);
                });
            }
        }
    });
    status()
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

async fn after_download() {
    let open = crate::account::signed_in() && core::get().store.lock().unwrap().config["onboarded"] == json!(true);
    if open {
        use_it().await;
    }
}

async fn install(p: &Part, base: u64) -> Result<(), String> {
    let dir = root();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(if p.zip { format!("{}.zip.part", p.id) } else { format!("{MODEL_FILE}.part") });
    fetch_to(p, &tmp, base).await?;
    report(|g| g.checking = true);
    let path = tmp.clone();
    let sum = tokio::task::spawn_blocking(move || sha256_file(&path)).await.ok().flatten().unwrap_or_default();
    report(|g| g.checking = false);
    if sum != p.sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{} did not download correctly. Try again.", p.label));
    }
    if p.zip {
        let (zip, dest) = (tmp.clone(), dir.join("whisper"));
        let label = p.label;
        tokio::task::spawn_blocking(move || unzip(&zip, &dest)).await.ok().flatten().ok_or(format!("could not unpack {label}"))?;
        let _ = std::fs::remove_file(&tmp);
    } else {
        std::fs::rename(&tmp, dir.join(MODEL_FILE)).map_err(|e| e.to_string())?;
    }
    std::fs::write(dir.join(format!("{}.ok", p.id)), p.sha256).map_err(|e| e.to_string())
}

// Downloads one file, picking up where an earlier try stopped
async fn fetch_to(p: &Part, file: &Path, base: u64) -> Result<(), String> {
    let have = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    if have >= p.size {
        return Ok(());
    }
    let mut req = crate::CLIENT.get(p.url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut r = req.send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("the download server answered {}", r.status().as_u16()));
    }
    let append = have > 0 && r.status().as_u16() == 206;
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(file)
        .map_err(|e| e.to_string())?;
    let (mut got, mut shown) = (if append { have } else { 0 }, Instant::now());
    while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
        if CANCEL.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        got += chunk.len() as u64;
        if got > p.size {
            return Err("the download was bigger than expected".into());
        }
        out.write_all(&chunk).map_err(|e| e.to_string())?;
        if shown.elapsed() > Duration::from_millis(250) {
            shown = Instant::now();
            report(|g| g.received = base + got);
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn unzip(zip: &Path, dest: &Path) -> Option<()> {
    std::fs::create_dir_all(dest).ok()?;
    let tar = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32").join("tar.exe");
    let mut cmd = std::process::Command::new(tar);
    cmd.arg("-xf").arg(zip).arg("-C").arg(dest).args(["--strip-components", "1"]).args(whisper_files());
    no_window(&mut cmd);
    cmd.status().ok()?.success().then_some(())
}

fn no_window(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

// Files earlier versions downloaded for a formatting model that runs on this computer, which Murmint no longer has
fn drop_legacy() {
    let dir = root();
    for f in [
        "llama", "llama.ok", "Qwen2.5-Omni-3B-Q4_K_M.gguf", "format.ok", "s1-mini-q4_k_m.gguf", "s1-mini.ok",
        "SmolLM2-135M-Instruct-Q8_0.gguf", "smollm2-135m.ok", "gemma-3-270m-it-Q8_0.gguf", "gemma-3-270m.ok", "bench.json",
    ] {
        let p = dir.join(f);
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        let _ = std::fs::remove_file(dir.join(format!("{f}.part")));
    }
}

// ---- running it ----

struct Server {
    port: AtomicU64, // where whisper-server listens; 0 while it starts
    ready: AtomicBool,
    child: Mutex<Option<std::process::Child>>,
}
struct Running {
    server: Arc<Server>,
    proxy: tokio::task::JoinHandle<()>,
}
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn running() -> bool {
    RUNNING.lock().unwrap().as_ref().is_some_and(|r| r.server.ready.load(Ordering::SeqCst))
}

/// Starts Murmint 1 if it is downloaded and still set up under Models (the engine calls this when it starts).
pub async fn boot() {
    let listed = core::get().store.lock().unwrap().provider(PROVIDER).is_some();
    if installed() && listed {
        start().await;
    }
}

/// Starts Murmint 1 and points the engine at it; true once it answers.
pub async fn start() -> bool {
    drop_legacy();
    if !installed() {
        return false;
    }
    let _one = STARTING.lock().await;
    let server = RUNNING.lock().unwrap().as_ref().map(|r| r.server.clone());
    if let Some(server) = server {
        return wait_ready(&server, GENERATION.load(Ordering::SeqCst)).await;
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let key = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let Ok(listener) = TcpListener::bind("127.0.0.1:0").await else { return false };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return false };
    let server = Arc::new(Server { port: AtomicU64::new(0), ready: AtomicBool::new(false), child: Mutex::new(None) });
    let proxy = tokio::spawn(serve_proxy(listener, key.clone(), server.clone()));
    *RUNNING.lock().unwrap() = Some(Running { server: server.clone(), proxy });
    ui(
        "provider:save",
        &[json!({
            "id": PROVIDER, "name": "Murmint 1", "baseUrl": format!("http://127.0.0.1:{port}/v1"), "keyOptional": true,
            "local": true, "free": true,
            "note": "Murmint’s own speech model, running on this computer. Free, private and works offline.",
            "models": [{ "id": STT_MODEL, "label": "Murmint 1", "task": "stt", "tier": "bad", "price": { "perMinute": 0 } }],
        })],
    )
    .await;
    ui("provider:key", &[json!(PROVIDER), json!(key)]).await;
    launch(server.clone(), generation);
    let ok = wait_ready(&server, generation).await;
    if !ok {
        stop();
    }
    ok
}

/// Makes Murmint 1 the speech model, keeping what the person had as a fallback.
pub async fn use_it() -> bool {
    if !start().await {
        return false;
    }
    let was = core::get().store.lock().unwrap().config["models"]["stt"].clone();
    let mut keep: Vec<String> = vec![];
    for r in std::iter::once(&was["primary"]).chain(was["fallbacks"].as_array().into_iter().flatten()) {
        if let Some(r) = r.as_str().filter(|r| !r.is_empty() && !r.starts_with(&format!("{PROVIDER}::"))) {
            if !keep.iter().any(|k| k == r) {
                keep.push(r.to_string());
            }
        }
    }
    keep.truncate(3);
    let stt = json!({ "primary": format!("{PROVIDER}::{STT_MODEL}"), "fallbacks": keep, "routing": "manual" });
    ui("config:set", &[json!("models.stt"), stt]).await;
    true
}

// ipc::ui reaches this module too, so its calls from here are boxed
fn ui<'a>(ch: &'a str, a: &'a [Value]) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send + 'a>> {
    Box::pin(crate::ipc::ui(ch, a))
}

pub fn stop() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(r) = RUNNING.lock().unwrap().take() {
        r.proxy.abort();
        r.server.ready.store(false, Ordering::SeqCst);
        if let Some(mut c) = r.server.child.lock().unwrap().take() {
            let _ = c.kill();
        }
    }
}

// Runs the speech server on its own thread; one that crashes is restarted a few times
fn launch(server: Arc<Server>, generation: u64) {
    let dir = root();
    let rt = tokio::runtime::Handle::current();
    let threads = std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(2).max(2);
    std::thread::spawn(move || {
        let alive = || GENERATION.load(Ordering::SeqCst) == generation;
        for tries in 0..=3u64 {
            if tries > 0 {
                std::thread::sleep(Duration::from_millis(1500 * tries));
            }
            if !alive() {
                return;
            }
            let Some(port) = std::net::TcpListener::bind("127.0.0.1:0").ok().and_then(|l| l.local_addr().ok()).map(|a| a.port()) else {
                continue;
            };
            let exe = dir.join("whisper").join("whisper-server.exe");
            let mut cmd = std::process::Command::new(&exe);
            cmd.current_dir(dir.join("whisper"))
                .arg("-m")
                .arg(dir.join(MODEL_FILE))
                .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-l", "auto", "-t", &threads.to_string()])
                .args(["--inference-path", "/v1/audio/transcriptions"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            no_window(&mut cmd);
            let Ok(child) = cmd.spawn() else { continue };
            tie_to_engine(&child);
            server.port.store(port as u64, Ordering::SeqCst);
            *server.child.lock().unwrap() = Some(child);
            let s = server.clone();
            rt.spawn(async move {
                if poll_until_up(port).await && GENERATION.load(Ordering::SeqCst) == generation {
                    s.ready.store(true, Ordering::SeqCst);
                }
            });
            // wait for it to exit (or for stop() to take it away)
            loop {
                std::thread::sleep(Duration::from_millis(400));
                let mut slot = server.child.lock().unwrap();
                match slot.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(None)) => continue,
                    _ => {
                        slot.take();
                        break;
                    }
                }
            }
            server.ready.store(false, Ordering::SeqCst);
        }
    });
}

async fn poll_until_up(port: u16) -> bool {
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        let up = crate::CLIENT.get(format!("http://127.0.0.1:{port}/")).timeout(Duration::from_secs(2)).send().await;
        if up.is_ok_and(|r| r.status().as_u16() == 200) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    false
}

async fn wait_ready(server: &Server, generation: u64) -> bool {
    let until = Instant::now() + Duration::from_secs(62);
    while Instant::now() < until && GENERATION.load(Ordering::SeqCst) == generation {
        if server.ready.load(Ordering::SeqCst) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

// The server goes with the engine, even if the engine is killed
#[cfg(windows)]
fn tie_to_engine(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::*;
    static JOB: std::sync::OnceLock<Option<isize>> = std::sync::OnceLock::new();
    let job = JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        Some(job.0 as isize)
    });
    if let Some(job) = job {
        unsafe {
            let _ = AssignProcessToJobObject(HANDLE(*job as _), HANDLE(child.as_raw_handle() as _));
        }
    }
}
#[cfg(not(windows))]
fn tie_to_engine(_: &std::process::Child) {}

// ---- the proxy in front of it ----

async fn serve_proxy(listener: TcpListener, key: String, server: Arc<Server>) {
    let want = Arc::new(format!("Bearer {key}"));
    while let Ok((sock, _)) = listener.accept().await {
        tokio::spawn(proxy_one(sock, want.clone(), server.clone()));
    }
}

// reads what the client still has to send (up to 64 MB, for at most ten seconds) before answering it
async fn refuse(sock: &mut TcpStream, mut left: usize, code: u16, body: Value) {
    let mut sink = [0u8; 16384];
    let until = tokio::time::Instant::now() + Duration::from_secs(10);
    left = left.min(64 << 20);
    while left > 0 {
        match tokio::time::timeout_at(until, sock.read(&mut sink[..left.min(16384)])).await {
            Ok(Ok(n)) if n > 0 => left -= n,
            _ => break,
        }
    }
    answer(sock, code, body).await;
}

async fn answer(sock: &mut TcpStream, code: u16, body: Value) {
    let reason = match code {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        502 => "Bad Gateway",
        _ => "Service Unavailable",
    };
    let body = body.to_string();
    let msg = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = sock.write_all(msg.as_bytes()).await;
}
fn fail(message: &str) -> Value {
    json!({ "error": { "message": message } })
}

// same length and same bytes, without stopping at the first difference
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn proxy_one(mut sock: TcpStream, want: Arc<String>, server: Arc<Server>) {
    let mut buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    let end = loop {
        let Ok(n) = sock.read(&mut chunk).await else { return };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 64 * 1024 {
            return;
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let target = head.split("\r\n").next().and_then(|l| l.split(' ').nth(1)).unwrap_or("").to_string();
    let header = |name: &str| {
        head.split("\r\n").skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    // an answer that isn't the server's still lets the client finish sending, so it reads the answer, not a reset
    let sent = buf.len() - end;
    let length: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let left = length.saturating_sub(sent);
    if !same(&header("authorization").unwrap_or_default(), &want) {
        return refuse(&mut sock, left, 401, fail("Wrong key")).await;
    }
    match target.split('?').next().unwrap_or("") {
        "/v1/models" => return refuse(&mut sock, left, 200, json!({ "object": "list", "data": [{ "id": STT_MODEL, "object": "model" }] })).await,
        "/v1/audio/transcriptions" => {}
        _ => return refuse(&mut sock, left, 404, fail("Not found")).await,
    }
    let port = server.port.load(Ordering::SeqCst);
    if !server.ready.load(Ordering::SeqCst) || port == 0 {
        return refuse(&mut sock, left, 503, fail("Murmint 1 is still starting. Try again in a moment.")).await;
    }
    let Ok(mut up) = TcpStream::connect(("127.0.0.1", port as u16)).await else {
        return refuse(&mut sock, left, 502, fail("Murmint 1 stopped. It restarts on its own; try again in a moment.")).await;
    };
    let head = head
        .split("\r\n")
        .map(|l| if l.split_once(':').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("host")) { format!("Host: 127.0.0.1:{port}") } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\r\n");
    if up.write_all(head.as_bytes()).await.is_err() || up.write_all(&buf[end..]).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut sock, &mut up).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_compare_whole() {
        assert!(same("Bearer abc", "Bearer abc"));
        assert!(!same("Bearer abd", "Bearer abc"));
        assert!(!same("Bearer ab", "Bearer abc"));
    }

    #[test]
    fn the_server_files_come_out_of_the_release_folder() {
        let f = whisper_files();
        assert!(f.contains(&"Release/whisper-server.exe".to_string()));
        assert!(f.contains(&"Release/ggml-cpu-x64.dll".to_string()));
        assert_eq!(f.len(), 13);
    }
}
