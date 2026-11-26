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

    let mut dir = default_dir();
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
                        let installed = serde_json::to_string(&apps::installed())
                            .unwrap_or_else(|_| "[]".into());
                        let _ = webview.evaluate_script(&format!("setup.apps({installed})"));
                        window.set_visible(true);
                    }
                    "drag" => {
                        let _ = window.drag_window();
                    }
                    "min" => window.set_minimized(true),
                    "close" if !installing => *flow = ControlFlow::Exit,
                    "browse" => {
                        if let Some(picked) = win::pick_folder(window.hwnd()) {
                            dir = if Path::new(&picked)
                                .file_name()
                                .map_or(false, |n| n.eq_ignore_ascii_case("Murmint"))
                            {
                                picked
                            } else {
                                format!("{}\\Murmint", picked.trim_end_matches('\\'))
                            };
                            let _ =
                                webview.evaluate_script(&format!("setup.dir({})", js_str(&dir)));
                        }
                    }
                    "install" if !installing && !installed => {
                        installing = true;
                        if let Some(d) = v["dir"].as_str().filter(|d| !d.trim().is_empty()) {
                            dir = d.to_string();
                        }
                        let (target, desktop, p) = (
                            dir.clone(),
                            v["desktop"].as_bool().unwrap_or(true),
                            proxy.clone(),
                        );
                        std::thread::spawn(move || {
                            let ok = install(&target, desktop, &p);
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

static OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn ok_flag() -> bool {
    OK.load(std::sync::atomic::Ordering::SeqCst)
}

fn install(
    dir: &str,
    desktop: bool,
    p: &tao::event_loop::EventLoopProxy<Ev>,
) -> Result<(), String> {
    let say = |pct: f64, status: &str| {
        let _ = p.send_event(Ev::Js(format!(
            "setup.progress({pct:.1},{})",
            js_str(status)
        )));
    };
    say(2.0, "Preparing");
    for exe in ["Murmint.exe", "murmint-engine.exe"] {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/IM", exe])
            .creation_flags(NO_WINDOW)
            .status();
    }
    let setup = payload_path().map_err(|e| format!("Could not unpack the installer: {e}"))?;
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
