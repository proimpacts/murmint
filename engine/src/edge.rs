//! Murmint Edge: a smaller, faster speech model than Murmint 1, built on SenseVoice Small (int8) and run by
//! sherpa-onnx on this computer. Like Murmint 1 it is free, private and works offline.
//!
//! Every file is pinned to one version and checked against its SHA-256 before it is used. Each dictation runs the
//! recogniser once on a temporary WAV file: nothing listens on the network, so nothing else on this computer or the
//! local network can reach it.

use crate::core;
use crate::murmint1::{free_space, gb, no_window, root, sha256_file};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const PROVIDER: &str = "murmint-edge";
pub const STT_MODEL: &str = "sensevoice-small";
const SPARE: u64 = 60 * 1024 * 1024; // room for the archive while it unpacks
const RECOGNISER: &str = "sherpa-onnx-offline.exe";

struct Part {
    id: &'static str,
    label: &'static str,
    url: &'static str,
    size: u64,
    sha256: &'static str,
    /// the file it becomes in the edge folder, or the folder the archive unpacks into
    file: &'static str,
    archive: bool,
}

const PARTS: [Part; 3] = [
    Part {
        id: "runtime",
        label: "Speech engine",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.3/sherpa-onnx-v2.0.3-win-x64-shared-MT-Release-no-tts.tar.bz2",
        size: 30850705,
        sha256: "e1536db1fcd1dde829f3cf26dfdbf60909aee6984983f6cd16b03b1352ac43fc",
        file: "runtime",
        archive: true,
    },
    Part {
        id: "model",
        label: "Murmint Edge",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07/model.int8.onnx",
        size: 239233841,
        sha256: "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
        file: "model.int8.onnx",
        archive: false,
    },
    Part {
        id: "tokens",
        label: "Murmint Edge",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07/tokens.txt",
        size: 315894,
        sha256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
        file: "tokens.txt",
        archive: false,
    },
];
const TOTAL: u64 = PARTS[0].size + PARTS[1].size + PARTS[2].size;

// only the recogniser and the ONNX Runtime it loads come out of the archive
const RUNTIME_FILES: [&str; 3] = [RECOGNISER, "onnxruntime.dll", "onnxruntime_providers_shared.dll"];

fn dir() -> PathBuf {
    root().join("edge")
}
fn non_empty(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}
fn done_in(dir: &Path, p: &Part) -> bool {
    let ok_path = dir.join(format!("{}.ok", p.id));
    if !std::fs::read_to_string(&ok_path).is_ok_and(|s| s == p.sha256) {
        return false;
    }
    let present = if p.archive {
        RUNTIME_FILES.iter().all(|f| non_empty(&dir.join(p.file).join(f)))
    } else {
        non_empty(&dir.join(p.file))
    };
    if !present {
        let _ = std::fs::remove_file(&ok_path);
    }
    present
}
pub fn installed() -> bool {
    core::try_get().is_some() && PARTS.iter().all(|p| done_in(&dir(), p))
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
static DL_GEN: AtomicU64 = AtomicU64::new(0);
static DL_LOCK: Mutex<()> = Mutex::new(());

fn cancelled(gen: u64) -> bool {
    DL_GEN.load(Ordering::SeqCst) != gen
}

fn status_of(p: &Progress) -> Value {
    let installed = installed();
    let state = if DOWNLOADING.load(Ordering::SeqCst) { "downloading" } else if installed { "installed" } else { p.state };
    json!({ "model": "edge", "state": state, "received": p.received, "total": TOTAL, "error": p.error,
            "checking": p.checking, "installed": installed, "size": TOTAL })
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
    if core::try_get().is_some() {
        core::push("local:progress", "main", status_of(&p));
    }
}

/// Downloads Murmint Edge in the background and lists it under Models when it is in place.
pub fn download() -> Value {
    let my_gen = {
        let _guard = DL_LOCK.lock().unwrap();
        if DOWNLOADING.swap(true, Ordering::SeqCst) {
            return status();
        }
        if installed() {
            DOWNLOADING.store(false, Ordering::SeqCst);
            tokio::spawn(list());
            return status();
        }
        let left: u64 = PARTS.iter().filter(|p| !done_in(&dir(), p)).map(|p| p.size).sum();
        if let Some(free) = free_space(&root()) {
            if free < left + SPARE {
                DOWNLOADING.store(false, Ordering::SeqCst);
                let msg = format!("Not enough disk space. Murmint Edge needs {} free and this drive has {}.", gb(left + SPARE), gb(free));
                report(|p| {
                    p.state = "error";
                    p.error = Some(msg);
                });
                return status();
            }
        }
        DL_GEN.fetch_add(1, Ordering::SeqCst) + 1
    };
    report(|p| {
        p.state = "downloading";
        p.error = None;
        p.received = 0;
        p.checking = false;
    });
    tokio::spawn(async move {
        let result = async {
            let mut base = 0;
            for p in &PARTS {
                if cancelled(my_gen) {
                    return Err("cancelled".to_string());
                }
                if !done_in(&dir(), p) {
                    install(p, base, my_gen).await?;
                }
                base += p.size;
                if !cancelled(my_gen) {
                    report(|g| g.received = base);
                }
            }
            Ok(())
        }
        .await;
        {
            let _guard = DL_LOCK.lock().unwrap();
            if cancelled(my_gen) {
                return;
            }
            DOWNLOADING.store(false, Ordering::SeqCst);
        }
        match result {
            Ok(()) => {
                report(|p| p.state = "installed");
                list().await;
            }
            Err(e) => report(|p| {
                p.checking = false;
                p.state = "error";
                p.error = Some(e);
            }),
        }
    });
    status()
}

pub fn cancel() {
    {
        let _guard = DL_LOCK.lock().unwrap();
        DL_GEN.fetch_add(1, Ordering::SeqCst);
        DOWNLOADING.store(false, Ordering::SeqCst);
    }
    report(|p| {
        if p.state == "downloading" {
            p.state = "none";
            p.error = None;
            p.checking = false;
        }
    });
}

/// Deletes Murmint Edge from this computer and takes it out of Models. Speech goes back to what was used before it.
pub async fn remove() -> Value {
    cancel();
    unlist().await;
    let _ = std::fs::remove_dir_all(dir());
    report(|p| {
        p.state = "none";
        p.received = 0;
        p.error = None;
    });
    status()
}

async fn install(p: &Part, base: u64, my_gen: u64) -> Result<(), String> {
    let dir = dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.{my_gen}.part", p.id));
    if let Err(e) = fetch_to(p, &tmp, base, my_gen).await {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if cancelled(my_gen) {
        let _ = std::fs::remove_file(&tmp);
        return Err("cancelled".into());
    }
    report(|g| g.checking = true);
    let path = tmp.clone();
    let sum = tokio::task::spawn_blocking(move || sha256_file(&path)).await.ok().flatten().unwrap_or_default();
    report(|g| g.checking = false);
    if sum != p.sha256 {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{} did not download correctly. Try again.", p.label));
    }
    if p.archive {
        let (archive, dest) = (tmp.clone(), dir.join(p.file));
        let ok = tokio::task::spawn_blocking(move || unpack(&archive, &dest)).await.ok().flatten().is_some();
        let _ = std::fs::remove_file(&tmp);
        if !ok {
            return Err(format!("Could not unpack the {}.", p.label.to_lowercase()));
        }
    } else if let Err(e) = std::fs::rename(&tmp, dir.join(p.file)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    std::fs::write(dir.join(format!("{}.ok", p.id)), p.sha256).map_err(|e| e.to_string())
}

// Downloads one file, picking up where an earlier try stopped
async fn fetch_to(p: &Part, file: &Path, base: u64, my_gen: u64) -> Result<(), String> {
    let have = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    if have >= p.size {
        return Ok(());
    }
    let mut req = crate::CLIENT.get(p.url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let step = Duration::from_secs(30);
    let mut r = tokio::time::timeout(step, req.send()).await.map_err(|_| "The download timed out.")?.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("The download server answered {}.", r.status().as_u16()));
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
    while let Some(chunk) = tokio::time::timeout(step, r.chunk()).await.map_err(|_| "The download timed out.")?.map_err(|e| e.to_string())? {
        if cancelled(my_gen) {
            return Err("cancelled".into());
        }
        got += chunk.len() as u64;
        if got > p.size {
            return Err("The download was bigger than expected.".into());
        }
        out.write_all(&chunk).map_err(|e| e.to_string())?;
        if shown.elapsed() > Duration::from_millis(250) {
            shown = Instant::now();
            report(|g| g.received = base + got);
        }
    }
    Ok(())
}

// Windows' own tar unpacks .tar.bz2; the files sit in <release>/bin inside it
fn unpack(archive: &Path, dest: &Path) -> Option<()> {
    std::fs::create_dir_all(dest).ok()?;
    let tar = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32").join("tar.exe");
    let mut cmd = std::process::Command::new(tar);
    cmd.arg("-xjf").arg(archive).arg("-C").arg(dest).args(["--strip-components", "2"]);
    let release = "sherpa-onnx-v2.0.3-win-x64-shared-MT-Release-no-tts";
    cmd.args(RUNTIME_FILES.iter().map(|f| format!("{release}/bin/{f}")));
    no_window(&mut cmd);
    cmd.status().ok()?.success().then_some(())
}

// ---- listing it under Models ----

fn ui<'a>(ch: &'a str, a: &'a [Value]) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send + 'a>> {
    Box::pin(crate::ipc::ui(ch, a))
}

/// Adds Murmint Edge to Models (the engine calls this at start too, so a deleted folder takes it back out).
pub async fn list() {
    if !installed() {
        if core::get().store.lock().unwrap().provider(PROVIDER).is_some() {
            unlist().await;
        }
        return;
    }
    ui(
        "provider:save",
        &[json!({
            "id": PROVIDER, "name": "Murmint Edge", "baseUrl": "http://127.0.0.1", "keyOptional": true,
            "local": true, "free": true,
            "note": "Murmint’s smaller, faster speech model, running on this computer. Free, private and works offline.",
            "models": [{ "id": STT_MODEL, "label": "Murmint Edge", "task": "stt", "tier": "good", "price": { "perMinute": 0 } }],
        })],
    )
    .await;
    // with nothing chosen for speech yet (Murmint 1 skipped at setup), Murmint Edge takes the job
    let primary = core::get().store.lock().unwrap().config["models"]["stt"]["primary"].clone();
    if primary.as_str().is_none_or(str::is_empty) {
        ui("config:set", &[json!("models.stt.primary"), json!(format!("{PROVIDER}::{STT_MODEL}"))]).await;
    }
}

async fn unlist() {
    let me = format!("{PROVIDER}::");
    let stt = core::get().store.lock().unwrap().config["models"]["stt"].clone();
    let uses_it = stt["primary"].as_str().is_some_and(|r| r.starts_with(&me));
    let fallbacks: Vec<Value> = stt["fallbacks"].as_array().into_iter().flatten().filter(|r| !r.as_str().is_some_and(|r| r.starts_with(&me))).cloned().collect();
    if uses_it || fallbacks.len() != stt["fallbacks"].as_array().map_or(0, |a| a.len()) {
        // the first fallback takes over; with none, Murmint 1 if it is here
        let mut rest = fallbacks.clone();
        let primary = if rest.is_empty() {
            if crate::murmint1::installed() { json!(format!("{}::whisper-tiny", crate::murmint1::PROVIDER)) } else { json!("") }
        } else {
            rest.remove(0)
        };
        let primary = if uses_it { primary } else { stt["primary"].clone() };
        let rest = if uses_it { rest } else { fallbacks };
        ui("config:set", &[json!("models.stt.primary"), primary]).await;
        ui("config:set", &[json!("models.stt.fallbacks"), json!(rest)]).await;
    }
    ui("provider:remove", &[json!(PROVIDER)]).await;
}

// ---- transcribing ----

/// Turns one dictation (a WAV file) into text. `language` is a code like "en", or None for automatic.
pub async fn transcribe(wav: &[u8], language: Option<&str>) -> Result<String, String> {
    if !installed() {
        return Err("Murmint Edge isn't downloaded. Download it under Models.".into());
    }
    let lang = match language.map(|l| l.to_ascii_lowercase()) {
        Some(l) if ["en", "zh", "ja", "ko", "yue"].contains(&l.as_str()) => l,
        _ => "auto".to_string(),
    };
    let dir = dir();
    let wav_path = std::env::temp_dir().join(format!("murmint-edge-{}.wav", uuid::Uuid::new_v4().simple()));
    std::fs::write(&wav_path, wav).map_err(|e| e.to_string())?;
    let threads = std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(2).clamp(1, 4);
    let mut cmd = std::process::Command::new(dir.join("runtime").join(RECOGNISER));
    cmd.current_dir(dir.join("runtime"))
        .arg(format!("--sense-voice-model={}", dir.join("model.int8.onnx").display()))
        .arg(format!("--tokens={}", dir.join("tokens.txt").display()))
        .arg(format!("--sense-voice-language={lang}"))
        .arg("--sense-voice-use-itn=1")
        .arg(format!("--num-threads={threads}"))
        .arg(&wav_path)
        .stdin(std::process::Stdio::null());
    no_window(&mut cmd);
    let out = tokio::time::timeout(Duration::from_secs(120), tokio::task::spawn_blocking(move || cmd.output())).await;
    let _ = std::fs::remove_file(&wav_path);
    let out = out
        .map_err(|_| "Murmint Edge took too long.".to_string())?
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Murmint Edge could not start: {e}"))?;
    let printed = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    parse_result(&printed).ok_or_else(|| "Murmint Edge didn't return any text.".into())
}

// The recogniser prints its settings, then one JSON line per file with the text in it
fn parse_result(printed: &str) -> Option<String> {
    printed.lines().find_map(|l| {
        let l = l.trim();
        if !l.starts_with('{') {
            return None;
        }
        let v: Value = serde_json::from_str(l).ok()?;
        v["text"].as_str().map(|t| t.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_is_read_from_the_result_line() {
        let printed = "OfflineRecognizerConfig(...)\nDone!\n\nC:\\tmp\\a.wav\n{\"lang\": \"<|en|>\", \"emotion\": \"<|NEUTRAL|>\", \"text\": \"Hello there.\", \"timestamps\": []}\n----\nnum threads: 2\n";
        assert_eq!(parse_result(printed).as_deref(), Some("Hello there."));
        assert_eq!(parse_result("no result here"), None);
    }

    #[test]
    fn done_needs_every_runtime_file_and_drops_stale_stamps() {
        let dir = std::env::temp_dir().join(format!("murmint-edge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("runtime")).unwrap();
        std::fs::write(dir.join("runtime.ok"), PARTS[0].sha256).unwrap();
        std::fs::write(dir.join("runtime").join(RECOGNISER), b"exe").unwrap();
        assert!(!done_in(&dir, &PARTS[0]));
        assert!(!dir.join("runtime.ok").exists());
        for f in RUNTIME_FILES {
            std::fs::write(dir.join("runtime").join(f), b"x").unwrap();
        }
        std::fs::write(dir.join("runtime.ok"), PARTS[0].sha256).unwrap();
        assert!(done_in(&dir, &PARTS[0]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
