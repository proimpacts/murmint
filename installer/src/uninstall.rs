#![windows_subsystem = "windows"]

mod shell;

use shell::{js_str, Ev, Shell, NO_WINDOW};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopProxy};
use wry::WebContext;

const HTML: &str = include_str!(concat!(env!("OUT_DIR"), "/uninstall.html"));

fn main() {
    // can't delete our own folder, so relaunch from temp
    let args: Vec<String> = std::env::args().collect();
    let dir = match args.iter().position(|a| a == "--from").and_then(|i| args.get(i + 1)) {
        Some(d) => PathBuf::from(d),
        None => {
            let Ok(me) = std::env::current_exe() else { return };
            let dir = me.parent().map(Path::to_path_buf).unwrap_or_default();
            let copy = std::env::temp_dir().join("murmint-uninstall.exe");
            if std::fs::copy(&me, &copy).is_ok() {
                let _ = Command::new(&copy).arg("--from").arg(&dir).spawn();
            }
            return;
        }
    };

    let mut ctx = WebContext::new(Some(std::env::temp_dir().join("murmint-setup-webview")));
    let Shell { event_loop, proxy, window, webview } = shell::open("Uninstall Murmint", HTML, &mut ctx);
    let Ok(webview) = webview else {
        let _ = Command::new(dir.join("Uninstall Murmint.exe")).spawn();
        return;
    };
    let mut working = false;
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } if !working => {
                remove_self_later();
                *flow = ControlFlow::Exit;
            }
            Event::UserEvent(Ev::Js(js)) => {
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(Ev::Ipc(msg)) => {
                let v: serde_json::Value = serde_json::from_str(&msg).unwrap_or_default();
                match v["cmd"].as_str().unwrap_or("") {
                    "ready" => window.set_visible(true),
                    "drag" => {
                        let _ = window.drag_window();
                    }
                    "min" => window.set_minimized(true),
                    "close" if !working => {
                        remove_self_later();
                        *flow = ControlFlow::Exit;
                    }
                    "uninstall" if !working => {
                        working = true;
                        let (dir, p) = (dir.clone(), proxy.clone());
                        let wipe = v["wipe"].as_bool().unwrap_or(false);
                        std::thread::spawn(move || {
                            let _ = p.send_event(Ev::Js(match uninstall(&dir, wipe, &p) {
                                Ok(()) => "setup.done()".into(),
                                Err(e) => format!("setup.failed({})", js_str(&e)),
                            }));
                            let _ = p.send_event(Ev::Ipc("{\"cmd\":\"_finished\"}".into()));
                        });
                    }
                    "_finished" => working = false,
                    _ => {}
                }
            }
            _ => {}
        }
    });
}

fn uninstall(dir: &Path, wipe: bool, p: &EventLoopProxy<Ev>) -> Result<(), String> {
    let say = |pct: f64, status: &str| {
        let _ = p.send_event(Ev::Js(format!("setup.progress({pct:.1},{})", js_str(status))));
    };
    say(8.0, "Closing Murmint");
    for exe in ["Murmint.exe", "murmint-engine.exe"] {
        let _ = Command::new("taskkill").args(["/F", "/T", "/IM", exe]).creation_flags(NO_WINDOW).status();
    }
    say(30.0, "Removing files");
    let regular = dir.join("Uninstall Murmint.exe");
    if regular.is_file() {
        // _?= runs in place and blocks
        let status = Command::new(&regular)
            .raw_arg("/S")
            .raw_arg(format!("_?={}", dir.display()))
            .creation_flags(NO_WINDOW)
            .status()
            .map_err(|e| format!("Could not start the uninstaller: {e}"))?;
        if !status.success() {
            return Err("Murmint couldn't be removed. Close it if it is open and try again.".into());
        }
    }
    say(80.0, "Tidying up");
    clean_install_dir(dir)?;
    restore_windows_clipboard();
    if let Ok(appdata) = std::env::var("APPDATA") {
        // the downloaded models and the sign-in go with the app, so a reinstall signs in again;
        // settings and history only when asked
        let data = Path::new(&appdata).join("Murmint");
        let _ = std::fs::remove_file(data.join("data").join("account.bin"));
        forget_sign_in(&data.join("data").join("secrets.json"));
        let target = if wipe { data } else { data.join("models") };
        if target.exists() {
            std::fs::remove_dir_all(&target)
                .map_err(|e| format!("Could not remove {}: {e}", target.display()))?;
        }
    }
    if wipe {
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("murmint-setup-webview"));
    }
    Ok(())
}

fn is_single_entry_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return false;
    }
    let mut comps = Path::new(name).components();
    matches!(comps.next(), Some(std::path::Component::Normal(_))) && comps.next().is_none()
}

fn read_manifest_entries(dir: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(dir.join(".murmint-install.json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let arr = v.get("entries").and_then(|e| e.as_array()).or_else(|| v.as_array());
    arr.map(|items| {
        items
            .iter()
            .filter_map(|x| x.as_str())
            .filter(|s| is_single_entry_name(s))
            .map(String::from)
            .collect()
    })
    .unwrap_or_default()
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

fn clean_install_dir(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    let manifest = read_manifest_entries(dir);
    let rd = std::fs::read_dir(dir)
        .map_err(|e| format!("Could not inspect {}: {e}", dir.display()))?;
    for entry in rd {
        let entry = entry.map_err(|e| format!("Could not read entry in {}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = entry
            .file_type()
            .map_err(|e| format!("Could not inspect {}: {e}", entry.path().display()))?;
        let owned = manifest.iter().any(|m| m.eq_ignore_ascii_case(&name))
            || is_known_murmint_entry(&name, ft.is_dir());
        if !owned {
            continue;
        }
        let path = entry.path();
        if ft.is_symlink() {
            let res = if ft.is_dir() {
                std::fs::remove_dir(&path)
            } else {
                std::fs::remove_file(&path)
            };
            res.map_err(|e| format!("Could not remove {}: {e}", path.display()))?;
        } else if ft.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("Could not remove {}: {e}", path.display()))?;
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Could not remove {}: {e}", path.display()))?;
        }
    }
    match std::fs::remove_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty || e.raw_os_error() == Some(145) => {
            Ok(())
        }
        Err(e) => {
            let non_empty = std::fs::read_dir(dir).is_ok_and(|mut r| r.next().is_some());
            if non_empty {
                Ok(())
            } else {
                Err(format!("Could not remove {}: {e}", dir.display()))
            }
        }
    }
}

// Better Clipboard switched Windows' own clipboard history off; put back the setting it had before.
fn restore_windows_clipboard() {
    let Ok(out) = Command::new("reg")
        .args(["query", r"HKCU\Software\Murmint", "/v", "ClipboardHistoryWas"])
        .creation_flags(NO_WINDOW)
        .output()
    else {
        return;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(hex) = text.split("0x").nth(1) else { return };
    let was = u32::from_str_radix(hex.trim().split_whitespace().next().unwrap_or("0"), 16).unwrap_or(0);
    let _ = Command::new("reg")
        .args(["add", r"HKCU\Software\Microsoft\Clipboard", "/v", "EnableClipboardHistory", "/t", "REG_DWORD", "/d", &was.to_string(), "/f"])
        .creation_flags(NO_WINDOW)
        .status();
    let _ = Command::new("reg").args(["delete", r"HKCU\Software\Murmint", "/v", "ClipboardHistoryWas", "/f"]).creation_flags(NO_WINDOW).status();
}

// the engine keeps the sign-in with the provider keys; only the sign-in goes, the keys stay with the settings
fn forget_sign_in(secrets: &Path) {
    let Ok(text) = std::fs::read_to_string(secrets) else { return };
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    if v.as_object_mut().and_then(|m| m.remove("murmint-account")).is_some() {
        let _ = std::fs::write(secrets, v.to_string());
    }
}

fn remove_self_later() {
    if let Ok(me) = std::env::current_exe() {
        let _ = Command::new("cmd")
            .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & del \"{}\"", me.display()))
            .creation_flags(NO_WINDOW)
            .spawn();
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
        let dir = std::env::temp_dir().join(format!("murmint-uninstall-test-{tag}-{nanos}"));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn clean_install_dir_preserves_unrelated_user_files_and_deletes_owned_entries() {
        let dir = temp_dir("keep-user");
        std::fs::write(dir.join("Murmint.exe"), b"exe").unwrap();
        std::fs::write(dir.join("Murmint Uninstaller.exe"), b"uninst").unwrap();
        std::fs::write(dir.join("ffmpeg.dll"), b"dll").unwrap();
        std::fs::create_dir_all(dir.join("resources")).unwrap();
        std::fs::write(dir.join("resources").join("app.asar"), b"asar").unwrap();
        std::fs::write(
            dir.join(".murmint-install.json"),
            serde_json::json!({ "version": 1, "entries": ["Murmint.exe", "custom.txt", "../escape.txt"] }).to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("custom.txt"), b"owned").unwrap();

        std::fs::write(dir.join("user-notes.txt"), b"keep").unwrap();
        std::fs::create_dir_all(dir.join("user-folder")).unwrap();
        std::fs::write(dir.join("user-folder").join("data.txt"), b"keep2").unwrap();

        clean_install_dir(&dir).unwrap();

        assert!(!dir.join("Murmint.exe").exists());
        assert!(!dir.join("Murmint Uninstaller.exe").exists());
        assert!(!dir.join("ffmpeg.dll").exists());
        assert!(!dir.join("resources").exists());
        assert!(!dir.join(".murmint-install.json").exists());
        assert!(!dir.join("custom.txt").exists());
        assert!(dir.join("user-notes.txt").exists());
        assert!(dir.join("user-folder").join("data.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_install_dir_removes_empty_root() {
        let dir = temp_dir("empty-root");
        std::fs::write(dir.join("Murmint.exe"), b"exe").unwrap();
        std::fs::write(dir.join("Uninstall Murmint.exe"), b"nsis").unwrap();
        clean_install_dir(&dir).unwrap();
        assert!(!dir.exists());
    }
}
