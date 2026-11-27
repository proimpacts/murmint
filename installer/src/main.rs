#![windows_subsystem = "windows"]

mod apps;
mod shell;

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use shell::{js_str, Ev, Shell, NO_WINDOW};
use tao::event::{Event, WindowEvent};
use tao::event_loop::ControlFlow;
use tao::platform::windows::WindowExtWindows;
use wry::WebContext;

static PAYLOAD: &[u8] = include_bytes!(env!("MURMINT_PAYLOAD"));
static UNINSTALLER: &[u8] = include_bytes!(env!("MURMINT_UNINSTALLER"));
const UNPACKED: &str = env!("MURMINT_UNPACKED_BYTES");
const HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/setup.html"));

fn default_dir() -> String {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        format!(
            "{}\\AppData\\Local",
            std::env::var("USERPROFILE").unwrap_or_default()
        )
    });
    format!("{base}\\Programs\\Murmint")
}

fn payload_path() -> std::io::Result<PathBuf> {
    let p = std::env::temp_dir().join("murmint-setup-core.exe");
    std::fs::write(&p, PAYLOAD)?;
    Ok(p)
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            _ => 0,
        })
        .sum()
}

fn main() {
    // an update started by Murmint: straight to the progress page, installed over the old one (settings and history
    // live elsewhere, so they stay), then Murmint starts again
    let args: Vec<String> = std::env::args().collect();
    let update = args.iter().any(|a| a == "--update");
    let update_version = args.iter().find_map(|a| a.strip_prefix("--version=")).unwrap_or("").to_string();
    let mut ctx = WebContext::new(Some(std::env::temp_dir().join("murmint-setup-webview")));
    let Shell { event_loop, proxy, window, webview } = shell::open("Murmint Setup", HTML, &mut ctx);
    let webview = match webview {
        Ok(w) => w,
        Err(e) => {
            let _ = std::fs::write(std::env::temp_dir().join("murmint-setup-error.txt"), format!("{e:?}"));
            if let Ok(p) = payload_path() {
                let _ = Command::new(p).spawn();
            }
            return;
        }
    };

    let mut dir = args.iter().find_map(|a| a.strip_prefix("--dir=")).map(String::from).filter(|_| update).unwrap_or_else(default_dir);
    if !update {
        dir = ensure_safe_dir(&dir);
    }
    let mut installing = false;
    let mut installed = false;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } if !installing => *flow = ControlFlow::Exit,
            Event::UserEvent(Ev::Js(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(Ev::Ipc(msg)) => {
                let v: serde_json::Value = serde_json::from_str(&msg).unwrap_or_default();
                match v["cmd"].as_str().unwrap_or("") {
                    "ready" => {
                        let _ = webview.evaluate_script(&format!("setup.dir({})", js_str(&dir)));
                        let found_apps = serde_json::to_string(&apps::installed())
                            .unwrap_or_else(|_| "[]".into());
                        let _ = webview.evaluate_script(&format!("setup.apps({found_apps})"));
                        if update && !installing && !installed {
                            installing = true;
                            let _ = webview.evaluate_script(&format!("setup.updating({})", js_str(&update_version)));
                            let (target, p) = (dir.clone(), proxy.clone());
                            std::thread::spawn(move || {
                                let ok = install(&target, true, Some(&p));
                                let _ = p.send_event(Ev::Js(match &ok {
                                    Ok(()) => "setup.progress(100,'Opening Murmint')".into(),
                                    Err(e) => format!("setup.failed({})", js_str(e)),
                                }));
                                let _ = p.send_event(Ev::Ipc(
                                    if ok.is_ok() { "{\"cmd\":\"_updated\"}" } else { "{\"cmd\":\"_failed\"}" }.into(),
                                ));
                            });
                        }
                        window.set_visible(true);
                    }
                    "_updated" => {
                        let _ = Command::new(Path::new(&dir).join("Murmint.exe")).current_dir(&dir).spawn();
                        *flow = ControlFlow::Exit;
                    }
                    "drag" => {
                        let _ = window.drag_window();
                    }
                    "min" => window.set_minimized(true),
                    "close" if !installing => *flow = ControlFlow::Exit,
                    "browse" => {
                        if let Some(picked) = win::pick_folder(window.hwnd()) {
                            let candidate = if Path::new(&picked)
                                .file_name()
                                .map_or(false, |n| n.eq_ignore_ascii_case("Murmint"))
                            {
                                picked
                            } else {
                                format!("{}\\Murmint", picked.trim_end_matches('\\'))
                            };
                            dir = ensure_safe_dir(&candidate);
                            let _ =
                                webview.evaluate_script(&format!("setup.dir({})", js_str(&dir)));
                        }
                    }
                    "install" if !installing && !installed => {
                        installing = true;
                        if let Some(d) = v["dir"].as_str().filter(|d| !d.trim().is_empty()) {
                            dir = d.trim().to_string();
                        }
                        if !update {
                            dir = ensure_safe_dir(&dir);
                            let _ =
                                webview.evaluate_script(&format!("setup.dir({})", js_str(&dir)));
                        }
                        let (target, desktop, p) = (
                            dir.clone(),
                            v["desktop"].as_bool().unwrap_or(true),
                            proxy.clone(),
                        );
                        std::thread::spawn(move || {
                            let ok = install(&target, desktop, Some(&p));
                            let _ = p.send_event(Ev::Js(if ok.is_ok() {
                                "setup.done()".into()
                            } else {
                                format!("setup.failed({})", js_str(&ok.unwrap_err()))
                            }));
                            let _ = p.send_event(Ev::Ipc(
                                if ok_flag() {
                                    "{\"cmd\":\"_installed\"}"
                                } else {
                                    "{\"cmd\":\"_failed\"}"
                                }
                                .into(),
                            ));
                        });
                    }
                    "_installed" => {
                        installing = false;
                        installed = true;
                    }
                    "_failed" => installing = false,
                    "finish" => {
                        if v["launch"].as_bool().unwrap_or(true) {
                            let _ = Command::new(Path::new(&dir).join("Murmint.exe"))
                                .current_dir(&dir)
                                .spawn();
                        }
                        *flow = ControlFlow::Exit;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    });
}

// point Installed apps at our uninstaller; it runs the NSIS one silently
fn register_uninstaller(dir: &str) {
    let exe = Path::new(dir).join("Murmint Uninstaller.exe");
    if std::fs::write(&exe, UNINSTALLER).is_err() {
        return;
    }
    if let Some(key) = uninstall_key() {
        let _ = Command::new("reg")
            .args(["add", &key, "/v", "UninstallString", "/t", "REG_SZ", "/f", "/d"])
            .arg(format!("\"{}\"", exe.display()))
            .creation_flags(NO_WINDOW)
            .status();
    }
}

/// The registry key Windows lists Murmint under in Installed apps.
fn uninstall_key() -> Option<String> {
    let out = Command::new("reg")
        .args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall", "/s", "/v", "DisplayName"])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()?;
    let mut key = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if line.starts_with("HKEY_") {
            key = Some(line.trim().to_string());
        } else if line.split_whitespace().collect::<Vec<_>>() == ["DisplayName", "REG_SZ", "Murmint"] {
            return key;
        }
    }
    None
}

fn has_murmint_marker(dir: &Path) -> bool {
    ["Murmint.exe", "Uninstall Murmint.exe", "Murmint Uninstaller.exe", ".murmint-install.json"]
        .iter()
        .any(|m| dir.join(m).exists())
}

fn is_non_empty_dir(dir: &Path) -> bool {
    dir.is_dir() && std::fs::read_dir(dir).is_ok_and(|mut rd| rd.next().is_some())
}

fn ensure_safe_dir(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches(['\\', '/']);
    let p = Path::new(trimmed);
    if is_non_empty_dir(p) && !has_murmint_marker(p) {
        format!("{trimmed}\\Murmint")
    } else {
        trimmed.to_string()
    }
}

fn is_known_murmint_entry(name: &str, is_dir: bool) -> bool {
    let exact = [
        "Murmint.exe",
        "Uninstall Murmint.exe",
        "Murmint Uninstaller.exe",
        "murmint-uninstall.exe",
        ".murmint-install.json",
    ];
    if exact.iter().any(|k| name.eq_ignore_ascii_case(k)) {
        return true;
    }
    if is_dir {
        return ["resources", "locales", "bin", "swiftshader"]
            .iter()
            .any(|k| name.eq_ignore_ascii_case(k));
    }
    if name.to_ascii_uppercase().starts_with("LICENSE") {
        return true;
    }
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            ["pak", "dll", "bin", "dat"]
                .iter()
                .any(|k| ext.eq_ignore_ascii_case(k))
        })
}

fn read_manifest_set(dir: &Path) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let Ok(raw) = std::fs::read_to_string(dir.join(".murmint-install.json")) else {
        return set;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return set;
    };
    if let Some(arr) = v.get("entries").and_then(|e| e.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                set.insert(s.to_ascii_lowercase());
            }
        }
    }
    set
}

fn snapshot_preexisting_other(dir: &Path) -> std::collections::HashSet<String> {
    let mut other = std::collections::HashSet::new();
    let prior = read_manifest_set(dir);
    let Ok(rd) = std::fs::read_dir(dir) else {
        return other;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().is_ok_and(|ft| ft.is_dir());
        let lower = name.to_ascii_lowercase();
        if !prior.contains(&lower) && !is_known_murmint_entry(&name, is_dir) {
            other.insert(lower);
        }
    }
    other
}

fn write_install_manifest(dir: &Path, preexisting_other: &std::collections::HashSet<String>) {
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".murmint-install.json" {
                continue;
            }
            if !preexisting_other.contains(&name.to_ascii_lowercase()) {
                entries.push(name);
            }
        }
    }
    entries.push(".murmint-install.json".to_string());
    entries.sort();
    entries.dedup();
    let manifest = serde_json::json!({
        "version": 1,
        "entries": entries,
    });
    let _ = std::fs::write(dir.join(".murmint-install.json"), manifest.to_string());
}

static OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn ok_flag() -> bool {
    OK.load(std::sync::atomic::Ordering::SeqCst)
}

fn install(
    dir: &str,
    desktop: bool,
    p: Option<&tao::event_loop::EventLoopProxy<Ev>>,
) -> Result<(), String> {
    let say = |pct: f64, status: &str| {
        if let Some(p) = p {
            let _ = p.send_event(Ev::Js(format!(
                "setup.progress({pct:.1},{})",
                js_str(status)
            )));
        }
    };
    say(2.0, "Preparing");
    for exe in ["Murmint.exe", "murmint-engine.exe"] {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/IM", exe])
            .creation_flags(NO_WINDOW)
            .status();
    }
    // taskkill returns before the processes are gone; the setup skips copying while Murmint is still running
    let running = || {
        Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Murmint.exe", "/NH"])
            .creation_flags(NO_WINDOW)
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("Murmint.exe"))
    };
    let start = Instant::now();
    while running() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_millis(500));
    let setup = payload_path().map_err(|e| format!("Could not unpack the installer: {e}"))?;
    let preexisting_other = snapshot_preexisting_other(Path::new(dir));
    say(6.0, "Copying files");
    let mut child = Command::new(&setup)
        .raw_arg("/S")
        .raw_arg(format!("/D={dir}"))
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Could not start the installer: {e}"))?;
    let expected = UNPACKED.parse::<f64>().unwrap_or(300e6).max(1.0);
    let (start, mut shown) = (Instant::now(), 6.0f64);
    let code = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st;
        }
        let by_files = 6.0 + 88.0 * (dir_size(Path::new(dir)) as f64 / expected).min(1.0);
        let by_time = 6.0 + 88.0 * (1.0 - (-start.elapsed().as_secs_f64() / 12.0).exp());
        shown = shown
            .max(by_files.min(94.0))
            .max(by_time.min(90.0) * 0.6 + shown * 0.4);
        say(
            shown.min(94.0),
            if shown < 80.0 {
                "Copying files"
            } else {
                "Finishing"
            },
        );
        std::thread::sleep(Duration::from_millis(180));
    };
    let _ = std::fs::remove_file(&setup);
    if !code.success() {
        return Err(format!(
            "The installer stopped (code {}). Close Murmint if it is open and try again.",
            code.code().unwrap_or(-1)
        ));
    }
    say(97.0, "Finishing");
    register_uninstaller(dir);
    write_install_manifest(Path::new(dir), &preexisting_other);
    if !desktop {
        if let Some(d) = win::desktop_dir() {
            let _ = std::fs::remove_file(Path::new(&d).join("Murmint.lnk"));
        }
    }
    OK.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

mod win {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FileOpenDialog, IFileOpenDialog, SHGetKnownFolderPath,
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, KNOWN_FOLDER_FLAG, SIGDN_FILESYSPATH,
    };

    fn take(p: PWSTR) -> Option<String> {
        unsafe {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as _));
            s
        }
    }

    pub fn pick_folder(owner: isize) -> Option<String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let d: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            d.SetOptions(d.GetOptions().ok()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
                .ok()?;
            d.SetTitle(windows::core::w!("Choose where to install Murmint"))
                .ok()?;
            d.Show(Some(HWND(owner as _))).ok()?;
            take(d.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?)
        }
    }

    pub fn desktop_dir() -> Option<String> {
        unsafe { take(SHGetKnownFolderPath(&FOLDERID_Desktop, KNOWN_FOLDER_FLAG(0), None).ok()?) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("murmint-setup-test-{tag}-{nanos}"));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn ensure_safe_dir_appends_murmint_only_for_non_empty_unmarked_dirs() {
        let dir = temp_dir("nonempty");
        std::fs::write(dir.join("unrelated.txt"), b"x").unwrap();
        assert_eq!(ensure_safe_dir(&dir.to_string_lossy()), format!("{}\\Murmint", dir.display()));

        std::fs::write(dir.join("Murmint.exe"), b"exe").unwrap();
        assert_eq!(ensure_safe_dir(&dir.to_string_lossy()), dir.to_string_lossy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_install_manifest_excludes_preexisting_user_files() {
        let dir = temp_dir("manifest");
        std::fs::write(dir.join("user.txt"), b"keep").unwrap();
        let pre = snapshot_preexisting_other(&dir);
        std::fs::write(dir.join("Murmint.exe"), b"exe").unwrap();
        std::fs::create_dir_all(dir.join("resources")).unwrap();
        write_install_manifest(&dir, &pre);

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(".murmint-install.json")).unwrap()).unwrap();
        let entries: Vec<&str> = v["entries"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert!(entries.contains(&"Murmint.exe"));
        assert!(entries.contains(&"resources"));
        assert!(entries.contains(&".murmint-install.json"));
        assert!(!entries.contains(&"user.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
