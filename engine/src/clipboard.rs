//! Better Clipboard: notices what you copy, and takes over Win+V so Murmint's clipboard opens instead of Windows'.
//! Copies marked private (password managers, and every paste and clipboard restore Murmint makes) are never kept.
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Better Clipboard is on: copies are reported and Win+V is taken over.
pub static ON: AtomicBool = AtomicBool::new(false);
/// The clipboard's change number right after Murmint last wrote to it, so that write is not reported as a copy.
static OWN: AtomicU32 = AtomicU32::new(0);

pub fn mark_own() {
    OWN.store(win::sequence(), Ordering::Relaxed);
}

/// The first animated GIF an HTML fragment shows, such as a GIF copied from a web page.
pub fn gif_in_html(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("<img") {
        let tag_start = at + i;
        let tag_end = lower[tag_start..].find('>').map_or(lower.len(), |e| tag_start + e);
        let tag = &html[tag_start..tag_end];
        if let Some(src) = attr(tag, "src") {
            let path = src.split(['?', '#']).next().unwrap_or("").to_lowercase();
            if src.starts_with("https://") && path.ends_with(".gif") {
                return Some(src);
            }
        }
        at = tag_end;
    }
    None
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let i = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[i..];
    let (q, rest) = match rest.chars().next()? {
        c @ ('"' | '\'') => (c, &rest[1..]),
        _ => (' ', rest),
    };
    Some(rest[..rest.find(q).unwrap_or(rest.len())].replace("&amp;", "&"))
}

/// Watches for copies and listens for Win+V (and Win+. or Win+; for the emoji page). `emit` gets ("clipboard:copied", copy) and ("clipboard:open", target).
pub fn start(emit: impl Fn(&str, Value) + Send + Sync + 'static) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let emit = std::sync::Arc::new(emit);
        let on_copy = emit.clone();
        std::thread::Builder::new()
            .name("clipboard-watch".into())
            .spawn(move || {
                let mut last = win::sequence();
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let seq = win::sequence();
                    if seq == last {
                        continue;
                    }
                    last = seq;
                    if !ON.load(Ordering::Relaxed) || seq == OWN.load(Ordering::Relaxed) {
                        continue;
                    }
                    // let the app that copied finish writing every format
                    std::thread::sleep(std::time::Duration::from_millis(80));
                    if let Some(copy) = win::read_copy() {
                        on_copy("clipboard:copied", copy);
                    }
                }
            })
            .ok();
        win::hook_win_v(move |target, tab| emit("clipboard:open", json!({ "hwnd": target, "tab": tab })));
    });
}

/// Puts files on the clipboard the way Explorer does when you copy them, so pasting a GIF sends the GIF itself.
pub fn write_files(paths: &[String]) -> bool {
    let ok = win::write_files(paths);
    mark_own();
    ok
}

/// Puts the window that was in front back in front, so a pick lands where you were typing.
pub fn refocus(hwnd: i64) {
    win::refocus(hwnd)
}

#[cfg(windows)]
mod win {
    use serde_json::{json, Value};
    use std::sync::atomic::Ordering;
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetForegroundWindow, GetMessageW, SetForegroundWindow, SetWindowsHookExW, KBDLLHOOKSTRUCT,
        MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    pub fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    fn open() -> bool {
        for _ in 0..10 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    unsafe fn bytes(fmt: u32) -> Option<Vec<u8>> {
        let h = GetClipboardData(fmt).ok()?;
        let hg = HGLOBAL(h.0);
        let size = GlobalSize(hg);
        if size == 0 || size > 8 * 1024 * 1024 {
            return None;
        }
        let p = GlobalLock(hg);
        if p.is_null() {
            return None;
        }
        let out = std::slice::from_raw_parts(p as *const u8, size).to_vec();
        let _ = GlobalUnlock(hg);
        Some(out)
    }

    fn wide(b: &[u8]) -> String {
        let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
        String::from_utf16_lossy(&w)
    }

    // CF_HDROP: a DROPFILES header, then the paths, each ending in a zero, with an extra zero at the end
    fn files(b: &[u8]) -> Vec<String> {
        if b.len() < 20 {
            return vec![];
        }
        let start = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let is_wide = u32::from_le_bytes([b[16], b[17], b[18], b[19]]) != 0;
        if !is_wide || start >= b.len() {
            return vec![];
        }
        let w: Vec<u16> = b[start..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        w.split(|&c| c == 0).take_while(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }

    /// What was just copied: text, files, a GIF link and whether there is a picture. None when it is private.
    pub fn read_copy() -> Option<Value> {
        if !open() {
            return None;
        }
        let out = unsafe {
            let private = ["ExcludeClipboardContentFromMonitorProcessing", "Clipboard Viewer Ignore"].iter().any(|n| {
                let name: Vec<u16> = n.encode_utf16().chain(Some(0)).collect();
                let f = RegisterClipboardFormatW(windows::core::PCWSTR(name.as_ptr()));
                f != 0 && IsClipboardFormatAvailable(f).is_ok()
            });
            let history_fmt = RegisterClipboardFormatW(windows::core::w!("CanIncludeInClipboardHistory"));
            let history_blocked = history_fmt != 0
                && IsClipboardFormatAvailable(history_fmt).is_ok()
                && super::history_blocked_by_bytes(bytes(history_fmt).as_deref());
            if private || history_blocked {
                None
            } else {
                let html_fmt = RegisterClipboardFormatW(windows::core::w!("HTML Format"));
                let text = bytes(13).map(|b| wide(&b)).filter(|t| !t.trim().is_empty());
                let files = bytes(15).map(|b| files(&b)).unwrap_or_default();
                let gif = (html_fmt != 0)
                    .then(|| bytes(html_fmt))
                    .flatten()
                    .and_then(|b| super::gif_in_html(&String::from_utf8_lossy(&b)));
                let image = IsClipboardFormatAvailable(8).is_ok() || IsClipboardFormatAvailable(17).is_ok();
                (text.is_some() || !files.is_empty() || gif.is_some() || image)
                    .then(|| json!({ "text": text, "files": files, "gif": gif, "image": image }))
            }
        };
        unsafe {
            let _ = CloseClipboard();
        }
        out
    }

    static OPEN: OnceLock<std::sync::mpsc::Sender<(i64, &'static str)>> = OnceLock::new();
    static mut SWALLOWED: u32 = 0;

    unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && super::ON.load(Ordering::Relaxed) {
            let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let msg = wparam.0 as u32;
            // V opens what was copied; . and ; (Windows' emoji keys) open the emoji page
            let tab = match k.vkCode {
                0x56 => Some("clips"),
                0xBE | 0xBA => Some("emoji"),
                _ => None,
            };
            if let Some(tab) = tab {
                let win_down = (GetAsyncKeyState(0x5B) as u16 & 0x8000) != 0 || (GetAsyncKeyState(0x5C) as u16 & 0x8000) != 0;
                if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && win_down {
                    if SWALLOWED != k.vkCode {
                        if let Some(tx) = OPEN.get() {
                            let _ = tx.send((GetForegroundWindow().0 as i64, tab));
                        }
                    }
                    SWALLOWED = k.vkCode;
                    return LRESULT(1);
                }
                if (msg == WM_KEYUP || msg == WM_SYSKEYUP) && SWALLOWED == k.vkCode {
                    SWALLOWED = 0;
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub fn hook_win_v(on_open: impl Fn(i64, &'static str) + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel::<(i64, &'static str)>();
        let _ = OPEN.set(tx);
        std::thread::Builder::new()
            .name("clipboard-open".into())
            .spawn(move || {
                for (hwnd, tab) in rx {
                    // Windows would open the Start menu when Win goes up after a swallowed key
                    crate::native::mask_start_menu();
                    on_open(hwnd, tab);
                }
            })
            .ok();
        std::thread::Builder::new()
            .name("clipboard-hook".into())
            .spawn(|| unsafe {
                if SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), None, 0).is_err() {
                    return;
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            })
            .ok();
    }

    // CF_HDROP: a DROPFILES header (20 bytes: offset to the paths, a point, two flags) then wide paths, double-zeroed
    pub fn write_files(paths: &[String]) -> bool {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
        use windows::Win32::System::Memory::{GlobalAlloc, GMEM_MOVEABLE};
        let mut data: Vec<u8> = vec![];
        data.extend(20u32.to_le_bytes());
        data.extend([0u8; 12]);
        data.extend(1u32.to_le_bytes());
        for p in paths {
            for w in p.encode_utf16().chain(Some(0)) {
                data.extend(w.to_le_bytes());
            }
        }
        data.extend([0u8, 0]);
        if !open() {
            return false;
        }
        unsafe {
            let _ = EmptyClipboard();
            if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, data.len()) {
                let p = GlobalLock(h);
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), p as *mut u8, data.len());
                    let _ = GlobalUnlock(h);
                    let _ = SetClipboardData(15, HANDLE(h.0));
                }
            }
            let _ = CloseClipboard();
        }
        true
    }

    pub fn refocus(hwnd: i64) {
        if hwnd != 0 {
            unsafe {
                let _ = SetForegroundWindow(HWND(hwnd as *mut core::ffi::c_void));
            }
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn sequence() -> u32 {
        0
    }
    pub fn read_copy() -> Option<serde_json::Value> {
        None
    }
    pub fn hook_win_v(_: impl Fn(i64, &'static str) + Send + 'static) {}
    pub fn refocus(_: i64) {}
    pub fn write_files(_: &[String]) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gif_copied_from_a_page_is_found_in_its_html() {
        let html = r#"<html><body><!--StartFragment--><img alt="cat" src="https://media.example.com/cat.gif?x=1&amp;y=2"><!--EndFragment--></body></html>"#;
        assert_eq!(gif_in_html(html).as_deref(), Some("https://media.example.com/cat.gif?x=1&y=2"));
        assert_eq!(gif_in_html(r#"<img src="https://example.com/photo.png">"#), None);
        assert_eq!(gif_in_html(r#"<img src="http://example.com/insecure.gif">"#), None);
        assert_eq!(gif_in_html("<p>no picture</p>"), None);
    }

    #[test]
    fn unicode_html_does_not_panic_and_finds_gifs() {
        assert_eq!(
            gif_in_html("İ<img src=\"https://example.com/a.gif\">").as_deref(),
            Some("https://example.com/a.gif")
        );
        assert_eq!(
            gif_in_html("KK<img src=\"https://example.com/b.gif\">").as_deref(),
            Some("https://example.com/b.gif")
        );
        assert_eq!(gif_in_html("İK<IMG ALT=\"İ\" SRC='https://example.com/c.GIF?v=1'>").as_deref(), Some("https://example.com/c.GIF?v=1"));
        assert_eq!(gif_in_html("İ<img src=\"https://example.com/"), None);
    }

    #[test]
    fn history_exclusion_bytes_block_zero_and_malformed_values() {
        assert!(history_blocked_by_bytes(None));
        assert!(history_blocked_by_bytes(Some(&[0, 0])));
        assert!(history_blocked_by_bytes(Some(&0u32.to_le_bytes())));
        assert!(!history_blocked_by_bytes(Some(&1u32.to_le_bytes())));
    }
}
