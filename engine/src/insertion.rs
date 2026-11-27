use crate::core;
use serde_json::{json, Value};
use std::time::Duration;

#[allow(dead_code)]
pub struct Inserted {
    pub ok: bool,
    pub method: &'static str,
    pub error: Option<String>,
}

#[cfg(windows)]
mod clip {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

    const CF_UNICODETEXT: u32 = 13;

    pub type Snapshot = Vec<(u32, Vec<u8>)>;

    fn open() -> bool {
        for _ in 0..10 {
            unsafe {
                if OpenClipboard(None).is_ok() {
                    return true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        false
    }

    pub fn snapshot() -> Snapshot {
        let mut out = vec![];
        if !open() {
            return out;
        }
        unsafe {
            let mut fmt = 0u32;
            loop {
                fmt = EnumClipboardFormats(fmt);
                if fmt == 0 {
                    break;
                }
                if !(fmt == CF_UNICODETEXT || fmt == 1 || fmt == 7 || fmt == 8 || fmt == 16 || fmt >= 0xC000)
                {
                    continue;
                }
                if fmt >= 0xC000 {
                    let mut n = [0u16; 64];
                    if GetClipboardFormatNameW(fmt, &mut n) == 0 {
                        continue;
                    }
                }
                let Ok(h) = GetClipboardData(fmt) else { continue };
                let hg = HGLOBAL(h.0);
                let size = GlobalSize(hg);
                if size == 0 || size > 64 * 1024 * 1024 {
                    continue;
                }
                let p = GlobalLock(hg);
                if !p.is_null() {
                    out.push((fmt, std::slice::from_raw_parts(p as *const u8, size).to_vec()));
                    let _ = GlobalUnlock(hg);
                }
            }
            let _ = CloseClipboard();
        }
        out
    }

    unsafe fn keep_out_of_history() {
        let dword = |v: u32| v.to_le_bytes();
        for (name, data) in [
            (windows::core::w!("ExcludeClipboardContentFromMonitorProcessing"), dword(0)),
            (windows::core::w!("CanIncludeInClipboardHistory"), dword(0)),
            (windows::core::w!("CanUploadToCloudClipboard"), dword(0)),
        ] {
            let fmt = RegisterClipboardFormatW(name);
            if fmt != 0 {
                set_raw(fmt, &data);
            }
        }
    }

    fn is_marker(fmt: u32) -> bool {
        let mut n = [0u16; 64];
        let len = unsafe { GetClipboardFormatNameW(fmt, &mut n) } as usize;
        let name = String::from_utf16_lossy(&n[..len.min(64)]);
        matches!(
            name.as_str(),
            "ExcludeClipboardContentFromMonitorProcessing"
                | "CanIncludeInClipboardHistory"
                | "CanUploadToCloudClipboard"
        )
    }

    pub fn restore(snap: &Snapshot) {
        if !open() {
            return;
        }
        unsafe {
            let _ = EmptyClipboard();
            for (fmt, bytes) in snap {
                if *fmt >= 0xC000 && is_marker(*fmt) {
                    continue;
                }
                set_raw(*fmt, bytes);
            }
            keep_out_of_history();
            let _ = CloseClipboard();
        }
    }

    unsafe fn set_raw(fmt: u32, bytes: &[u8]) {
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) {
            let p = GlobalLock(h);
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
                let _ = GlobalUnlock(h);
                let _ = SetClipboardData(fmt, HANDLE(h.0));
            }
        }
    }

    pub fn clear() {
        if open() {
            unsafe {
                let _ = EmptyClipboard();
                let _ = CloseClipboard();
            }
        }
    }

    pub fn write_text(text: &str) -> bool {
        let ok = write(text, false);
        // kept on the clipboard on purpose, but it is a dictation, not a copy
        crate::clipboard::mark_own();
        ok
    }
    pub fn write_text_private(text: &str) -> bool {
        write(text, true)
    }

    fn write(text: &str, private: bool) -> bool {
        if !open() {
            return false;
        }
        unsafe {
            let _ = EmptyClipboard();
            // Windows text boxes only show a line break written as CRLF
            let mut wide: Vec<u16> = text.replace("\r\n", "\n").replace('\n', "\r\n").encode_utf16().collect();
            wide.push(0);
            let bytes: Vec<u8> = wide.iter().flat_map(|w| w.to_le_bytes()).collect();
            set_raw(CF_UNICODETEXT, &bytes);
            if private {
                keep_out_of_history();
            }
            let _ = CloseClipboard();
        }
        true
    }

    pub fn read_text() -> Option<String> {
        if !open() {
            return None;
        }
        let out = unsafe {
            let r = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
                let hg = HGLOBAL(h.0);
                let p = GlobalLock(hg) as *const u16;
                if p.is_null() {
                    return None;
                }
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                let _ = GlobalUnlock(hg);
                Some(s)
            });
            let _ = CloseClipboard();
            r
        };
        out
    }
}

#[cfg(not(windows))]
mod clip {
    pub type Snapshot = Vec<(u32, Vec<u8>)>;
    pub fn snapshot() -> Snapshot {
        vec![]
    }
    pub fn restore(_: &Snapshot) {}
    pub fn clear() {}
    pub fn write_text(_: &str) -> bool {
        true
    }
    pub fn write_text_private(_: &str) -> bool {
        true
    }
    pub fn read_text() -> Option<String> {
        None
    }
}

pub use clip::write_text as write_clipboard_text;

#[cfg(test)]
pub static TEST_SINK: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

static LAST: std::sync::Mutex<Option<(String, &'static str)>> = std::sync::Mutex::new(None);

pub async fn target_editable() -> bool {
    #[cfg(test)]
    if TEST_SINK.lock().unwrap().is_some() {
        return std::env::var("MURMINT_TEST_EDITABLE").map_or(true, |v| v != "0");
    }
    let probe = tokio::task::spawn_blocking(crate::native::focus_editable);
    match tokio::time::timeout(std::time::Duration::from_millis(400), probe).await {
        Ok(Ok(Some(false))) => false,
        _ => true,
    }
}

pub async fn autotype() -> bool {
    #[cfg(test)]
    if TEST_SINK.lock().unwrap().is_some() {
        return std::env::var("MURMINT_TEST_AUTOTYPE").is_ok_and(|v| v == "1");
    }
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = cancelled.clone();
    let probe = tokio::task::spawn_blocking(move || crate::native::focus_text_box_cancellable(flag));
    match tokio::time::timeout(std::time::Duration::from_millis(1500), probe).await {
        Ok(Ok(true)) => {
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
            true
        }
        _ => {
            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            false
        }
    }
}

// held until the clipboard is restored, so back-to-back pastes don't save each other's text
static CLIPBOARD: std::sync::LazyLock<std::sync::Arc<tokio::sync::Mutex<()>>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Mutex::new(())));

#[derive(Debug, PartialEq, Eq)]
enum RestoreAction {
    Keep,
    Clear,
    Restore,
}

fn restore_action(snap: Option<&Option<clip::Snapshot>>) -> RestoreAction {
    match snap {
        Some(Some(s)) if s.is_empty() => RestoreAction::Clear,
        Some(Some(_)) => RestoreAction::Restore,
        _ => RestoreAction::Keep,
    }
}

fn restore_snap_sync(snap: &clip::Snapshot) {
    if snap.is_empty() {
        clip::clear();
    } else {
        clip::restore(snap);
    }
}

async fn restore_snap_async(snap: Option<Option<clip::Snapshot>>) {
    if let Some(Some(s)) = snap {
        tokio::task::spawn_blocking(move || restore_snap_sync(&s)).await.ok();
    }
}

/// Selected text in the foreground app (via Ctrl+C); restores the clipboard.
pub async fn copy_selection() -> Option<String> {
    let hold = CLIPBOARD.clone().lock_owned().await;
    let snap = tokio::task::spawn_blocking(clip::snapshot).await.ok().flatten();
    tokio::task::spawn_blocking(clip::clear).await.ok();
    crate::native::handle("native_copy", &json!({ "id": 0 })).await;
    let mut text = None;
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        if let Some(t) =
            tokio::task::spawn_blocking(clip::read_text).await.ok().flatten().filter(|t| !t.trim().is_empty())
        {
            text = Some(t);
            break;
        }
    }
    tokio::task::spawn_blocking(move || clip::restore(&snap)).await.ok();
    drop(hold);
    text
}

/// Discord limit is 2000.
const DISCORD_PASTE_LIMIT: usize = 1800;

async fn is_discord() -> bool {
    tokio::task::spawn_blocking(|| crate::native::foreground_full().0.to_lowercase().contains("discord"))
        .await
        .unwrap_or(false)
}

/// Split at newlines, then spaces.
fn split_for_paste(text: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut start = 0;
    while chars.len() - start > limit {
        let window = &chars[start..start + limit];
        let cut = window
            .iter()
            .rposition(|c| *c == '\n')
            .or_else(|| window.iter().rposition(|c| *c == ' '))
            .map(|i| i + 1)
            .unwrap_or(limit);
        out.push(chars[start..start + cut].iter().collect());
        start += cut;
    }
    out.push(chars[start..].iter().collect());
    out
}

/// Erases `n` characters before the cursor, for box joining.
pub async fn erase(n: usize) {
    #[cfg(test)]
    if let Some(sink) = TEST_SINK.lock().unwrap().as_mut() {
        sink.push(format!("<erase {n}>"));
        return;
    }
    let _ = tokio::task::spawn_blocking(move || crate::native::backspace(n)).await;
}

pub async fn insert(text: &str) -> Inserted {
    #[cfg(test)]
    if let Some(sink) = TEST_SINK.lock().unwrap().as_mut() {
        sink.push(text.to_string());
        return Inserted { ok: true, method: "test", error: None };
    }
    let cfg = { core::get().store.lock().unwrap().config["insertion"].clone() };
    // the placeholder for an image; as text it is only an "OBJ" box
    let mut text = text.replace('\u{fffc}', "");
    if cfg["trailingSpace"] == json!(true) && !text.ends_with(char::is_whitespace) {
        text.push(' ');
    }
    let restore = cfg["restoreClipboard"].as_bool().unwrap_or(true);
    let hold = CLIPBOARD.clone().lock_owned().await;
    let snap: Option<Option<clip::Snapshot>> = if restore {
        Some(tokio::task::spawn_blocking(clip::snapshot).await.ok().flatten())
    } else {
        None
    };
    let keep = cfg["method"] == "clipboard";
    let discord = if keep { false } else { is_discord().await };
    // Discord turns long pastes into a .txt upload
    let pieces = if discord { split_for_paste(&text, DISCORD_PASTE_LIMIT) } else { vec![] };
    if !target_matches(expected).await {
        drop(hold);
        return Inserted {
            ok: false,
            method: "unplaced",
            error: Some("Focus moved to another field".into()),
        };
    }
    {
        let t = if pieces.len() > 1 { pieces[0].clone() } else { text.clone() };
        let staged = tokio::task::spawn_blocking(
            move || if keep { clip::write_text(&t) } else { clip::write_text_private(&t) },
        )
        .await
        .unwrap_or(false);
        if !staged {
            restore_snap_async(snap).await;
            drop(hold);
            return Inserted {
                ok: false,
                method: "unplaced",
                error: Some("Couldn't write to the clipboard".into()),
            };
        }
    }

    if cfg["method"] == "clipboard" {
        *LAST.lock().unwrap() = Some((text, "clipboard", expected.cloned()));
        return Inserted { ok: true, method: "clipboard", error: None };
    }
    tokio::time::sleep(Duration::from_millis(25)).await;
    if pieces.len() > 1 {
        for (i, piece) in pieces.iter().enumerate() {
            if !target_matches(expected).await {
                restore_snap_async(snap).await;
                drop(hold);
                return Inserted {
                    ok: false,
                    method: "unplaced",
                    error: Some("Focus moved to another field".into()),
                };
            }
            if i > 0 {
                let p = piece.clone();
                let staged = tokio::task::spawn_blocking(move || clip::write_text_private(&p))
                    .await
                    .unwrap_or(false);
                if !staged {
                    restore_snap_async(snap).await;
                    drop(hold);
                    return Inserted {
                        ok: false,
                        method: "unplaced",
                        error: Some("Couldn't write to the clipboard".into()),
                    };
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
                if !target_matches(expected).await {
                    restore_snap_async(snap).await;
                    drop(hold);
                    return Inserted {
                        ok: false,
                        method: "unplaced",
                        error: Some("Focus moved to another field".into()),
                    };
                }
            }
            let r = crate::native::handle("native_paste", &json!({ "id": 0 })).await;
            if r.as_ref().map_or(true, |r| r["ok"] != json!(true)) {
                if i == 0 {
                    *LAST.lock().unwrap() = Some((text, "clipboard", expected.cloned()));
                }
                return Inserted {
                    ok: false,
                    method: "clipboard",
                    error: Some("Couldn't send Ctrl+V".into()),
                };
            }
            tokio::time::sleep(Duration::from_millis(140)).await;
        }
    } else {
        if !target_matches(expected).await {
            restore_snap_async(snap).await;
            drop(hold);
            return Inserted {
                ok: false,
                method: "unplaced",
                error: Some("Focus moved to another field".into()),
            };
        }
        let res = crate::native::handle("native_paste", &json!({ "id": 0 })).await;
        if res.as_ref().map_or(true, |r| r["ok"] != json!(true)) {
            *LAST.lock().unwrap() = Some((text, "clipboard", expected.cloned()));
            return Inserted { ok: false, method: "clipboard", error: Some("Couldn't send Ctrl+V".into()) };
        }
    }
    *LAST.lock().unwrap() = Some((text.clone(), "paste", expected.cloned()));
    if restore_action(snap.as_ref()) != RestoreAction::Keep {
        tokio::spawn(async move {
            // Slow apps (Discord, VS Code, browsers under load) read the clipboard a moment after Ctrl+V. Putting the old
            // clipboard back too soon made them paste that instead, so wait long enough for them to have read ours.
            tokio::time::sleep(Duration::from_millis(700)).await;
            let last_piece = split_for_paste(&text, DISCORD_PASTE_LIMIT).pop().unwrap_or_default();
            tokio::task::spawn_blocking(move || {
                let now = clip::read_text().map(|t| t.replace("\r\n", "\n"));
                let ours = [text, last_piece].map(|t| t.replace("\r\n", "\n"));
                if now.is_some_and(|t| ours.contains(&t)) {
                    if let Some(Some(s)) = snap {
                        restore_snap_sync(&s);
                    }
                }
            })
            .await
            .ok();
            drop(hold);
        });
    } else {
        drop(hold);
    }
    Inserted { ok: true, method: "paste", error: None }
}

pub async fn undo() -> Value {
    let last = LAST.lock().unwrap().clone();
    match last {
        Some((_, "paste", target)) => {
            if let Some(t) = target.as_ref() {
                if !target_matches(Some(t)).await {
                    return json!({ "ok": false, "error": "Focus moved since the last dictation" });
                }
            }
            let r = crate::native::handle("native_undo", &json!({ "id": 0 })).await;
            if r.is_some_and(|r| r["ok"] == json!(true)) {
                *LAST.lock().unwrap() = None;
                json!({ "ok": true })
            } else {
                json!({ "ok": false, "error": "Couldn't send Ctrl+Z" })
            }
        }
        _ => json!({ "ok": false, "error": "Nothing to undo" }),
    }
}

#[cfg(test)]
mod paste_split_tests {
    use super::split_for_paste;

    #[test]
    fn long_text_is_split_at_lines_or_spaces_and_nothing_is_lost() {
        let text = format!("{}\n{}", "word ".repeat(300), "more ".repeat(300));
        let pieces = split_for_paste(&text, 1800);
        assert!(pieces.len() >= 2);
        assert!(pieces.iter().all(|p| p.chars().count() <= 1800));
        assert_eq!(pieces.concat(), text);
        assert_eq!(split_for_paste("short", 1800), vec!["short".to_string()]);
    }

    #[test]
    fn restore_action_preserves_clipboard_when_restore_off_or_snapshot_failed() {
        assert_eq!(restore_action(None), RestoreAction::Keep);
        assert_eq!(restore_action(Some(&None)), RestoreAction::Keep);
        assert_eq!(restore_action(Some(&Some(vec![]))), RestoreAction::Clear);
        assert_eq!(restore_action(Some(&Some(vec![(15, vec![1, 2])]))), RestoreAction::Restore);
    }

    #[tokio::test]
    async fn target_mismatch_aborts_erase_insert_and_undo_without_mutating() {
        let _lock = TEST_GUARD.lock().unwrap();
        let _cleanup = TestCleanup;
        *TEST_SINK.lock().unwrap() = Some(vec![]);
        let target = crate::native::TargetIdentity {
            fg_hwnd: 123,
            focus_hwnd: 456,
            control_type: 50004,
            automation_id: "box".into(),
            class_name: "Edit".into(),
        };

        TEST_TARGET_MOVED.store(true, Ordering::SeqCst);
        assert!(!erase_in(3, Some(&target)).await);
        let ins = insert_into("secret text", Some(&target)).await;
        assert!(!ins.ok);
        assert_eq!(ins.method, "unplaced");
        assert_eq!(ins.error.as_deref(), Some("Focus moved to another field"));
        assert!(TEST_SINK.lock().unwrap().as_ref().unwrap().is_empty());

        *LAST.lock().unwrap() = Some(("pasted".into(), "paste", Some(target)));
        let u = undo().await;
        assert_eq!(u["ok"], json!(false));
        assert_eq!(u["error"], json!("Focus moved since the last dictation"));
        assert!(LAST.lock().unwrap().is_some());
    }
}
