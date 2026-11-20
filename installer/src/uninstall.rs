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
    let _ = std::fs::remove_dir_all(dir);
    restore_windows_clipboard();
    if let Ok(appdata) = std::env::var("APPDATA") {
        // the downloaded models and the sign-in go with the app, so a reinstall signs in again;
        // settings and history only when asked
        let data = Path::new(&appdata).join("Murmint");
        let _ = std::fs::remove_file(data.join("data").join("account.bin"));
        let _ = std::fs::remove_dir_all(if wipe { data } else { data.join("models") });
    }
    if wipe {
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("murmint-setup-webview"));
    }
    Ok(())
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

fn remove_self_later() {
    if let Ok(me) = std::env::current_exe() {
        let _ = Command::new("cmd")
            .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & del \"{}\"", me.display()))
            .creation_flags(NO_WINDOW)
            .spawn();
    }
}
