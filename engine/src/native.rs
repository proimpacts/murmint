use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

pub type Combos = HashMap<String, Vec<i32>>;

static COMBOS: LazyLock<Mutex<Combos>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn pick(combos: &Combos, down: &dyn Fn(i32) -> bool) -> Option<String> {
    let mut best: Option<(&String, usize)> = None;
    for (name, keys) in combos {
        if keys.is_empty() || !keys.iter().all(|k| down(*k)) {
            continue;
        }
        if best.map_or(true, |(_, n)| keys.len() > n) {
            best = Some((name, keys.len()));
        }
    }
    best.map(|(n, _)| n.clone())
}

pub struct Debounce {
    active: Option<String>,
    miss: u32,
}
pub enum Edge {
    Down(String),
    Up(String),
}
impl Debounce {
    pub fn new() -> Self {
        Debounce { active: None, miss: 0 }
    }
    pub fn step(&mut self, hit: Option<String>) -> Vec<Edge> {
        const NEED: u32 = 8;
        if hit.is_none() && self.active.is_some() {
            self.miss += 1;
            if self.miss < NEED {
                return vec![];
            }
        } else {
            self.miss = 0;
        }
        if hit == self.active {
            return vec![];
        }
        let mut out = vec![];
        if let Some(a) = self.active.take() {
            out.push(Edge::Up(a));
        }
        if let Some(h) = hit {
            self.active = Some(h.clone());
            out.push(Edge::Down(h));
        }
        out
    }
}

pub fn set_combos(v: &Value) {
    let mut m: Combos = HashMap::new();
    if let Some(o) = v.as_object() {
        for (k, arr) in o {
            let keys: Vec<i32> = arr
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_i64().map(|n| n as i32)).collect())
                .unwrap_or_default();
            m.insert(k.clone(), keys);
        }
    }
    *COMBOS.lock().unwrap() = m;
}

#[cfg(windows)]
mod win {
    use windows::core::{Interface, BSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::ProcessStatus::{K32EmptyWorkingSet, K32QueryWorkingSet};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    };
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    #[cfg(all(test, local_tests))]
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SendMessageW,
    };

    const WIN: i32 = 91;

    pub fn down(vk: i32) -> bool {
        unsafe {
            if vk == WIN {
                return (GetAsyncKeyState(0x5B) as u16 & 0x8000) != 0
                    || (GetAsyncKeyState(0x5C) as u16 & 0x8000) != 0;
            }
            (GetAsyncKeyState(vk) as u16 & 0x8000) != 0
        }
    }

    fn key(vk: u16, up: bool) {
        unsafe {
            keybd_event(vk as u8, 0, if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, 0);
        }
    }

    fn tap(k: u16) {
        key(0x11, false);
        key(k, false);
        key(k, true);
        key(0x11, true);
    }

    // Held modifiers are let go so the app sees a plain Ctrl+key. They are not pressed again afterwards: if the
    // person had already let go, Windows would keep them "held" and Backspace would become Ctrl+Backspace.
    fn with_modifiers_released(k: u16) {
        // letting go of Alt or Win with nothing in between opens the app's menu bar or the Start menu, and the
        // Ctrl+C or Ctrl+V after it would go there instead, so a harmless key goes first
        if [0x12, 0x5B, 0x5C].iter().any(|m| down(*m)) {
            mask_start_menu();
        }
        for m in [0x5Bu16, 0x5C, 0x12, 0x10, 0x11] {
            if down(m as i32) {
                key(m, true);
            }
        }
        tap(k);
    }
    pub fn paste() {
        with_modifiers_released(0x56);
    }
    pub fn copy() {
        with_modifiers_released(0x43);
    }

    pub fn undo() {
        with_modifiers_released(0x5A);
    }

    /// Presses Backspace `n` times, with no modifier held.
    pub fn backspace(n: usize) {
        for m in [0x5Bu16, 0x5C, 0x12, 0x10, 0x11] {
            if down(m as i32) {
                key(m, true);
            }
        }
        for _ in 0..n {
            key(0x08, false);
            key(0x08, true);
        }
    }

    pub fn mask_start_menu() {
        key(0xE8, false);
        key(0xE8, true);
    }

    static WIN_MASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    pub fn note_win(held: bool) {
        if held && !WIN_MASKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            mask_start_menu();
        }
        if !held {
            WIN_MASKED.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn pressed() -> Vec<i32> {
        let mut out = vec![];
        for vk in 8..=254 {
            if (0xA0..=0xA5).contains(&vk) || vk == 0x5C {
                continue;
            }
            if down(vk) {
                out.push(if vk == 0x5B { WIN } else { vk });
            }
        }
        out
    }

    pub fn private_memory_mb(pid: u32) -> f64 {
        unsafe {
            let opened =
                if pid == 0 { None } else { OpenProcess(PROCESS_QUERY_INFORMATION, false, pid).ok() };
            if pid != 0 && opened.is_none() {
                return 0.0;
            }
            let h = opened.unwrap_or_else(|| GetCurrentProcess());
            let r = private_mb_of(h);
            if opened.is_some() {
                let _ = CloseHandle(h);
            }
            r
        }
    }

    // Topmost over every app, but under the taskbar and the shell's own popups (Start, search, tray).
    pub fn raise_under_taskbar(overlay: isize) {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetClassNameW, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        const SHELL: &[&str] = &[
            "Windows.UI.Core.CoreWindow", "Shell_TrayWnd", "Shell_SecondaryTrayWnd", "NotifyIconOverflowWindow",
            "TopLevelWindowForOverflowXamlIsland", "Xaml_WindowedPopupClass", "XamlExplorerHostIslandWindow",
        ];
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        unsafe {
            let mut buf = [0u16; 64];
            let n = GetClassNameW(GetForegroundWindow(), &mut buf);
            if SHELL.contains(&String::from_utf16_lossy(&buf[..n.max(0) as usize]).as_str()) {
                return;
            }
            let _ = SetWindowPos(HWND(overlay as _), HWND_TOPMOST, 0, 0, 0, 0, flags);
            for class in ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
                let wide: Vec<u16> = class.encode_utf16().chain([0]).collect();
                if let Ok(tb) = FindWindowW(PCWSTR(wide.as_ptr()), PCWSTR::null()) {
                    let _ = SetWindowPos(tb, HWND_TOPMOST, 0, 0, 0, 0, flags);
                }
            }
        }
    }

    // trim the working set
    pub fn trim_working_set(pid: u32) -> bool {
        unsafe {
            if pid == 0 {
                return K32EmptyWorkingSet(GetCurrentProcess()).as_bool();
            }
            let Ok(h) = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_SET_QUOTA, false, pid) else {
                return false;
            };
            let ok = K32EmptyWorkingSet(h).as_bool();
            let _ = CloseHandle(h);
            ok
        }
    }

    unsafe fn private_mb_of(h: HANDLE) -> f64 {
        {
            let mut n = 2048usize;
            for _ in 0..4 {
                let mut buf = vec![0usize; n + 1];
                let ok = K32QueryWorkingSet(
                    h,
                    buf.as_mut_ptr() as *mut _,
                    (buf.len() * std::mem::size_of::<usize>()) as u32,
                )
                .as_bool();
                let entries = buf[0];
                if ok {
                    return buf[1..=entries.min(n)].iter().filter(|e| ((**e >> 5) & 7) <= 1).count() as f64
                        * 4096.0
                        / 1_048_576.0;
                }
                n = entries + 512;
            }
            0.0
        }
    }

    pub fn foreground_full() -> (String, String, String) {
        unsafe {
            let h: HWND = GetForegroundWindow();
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(h, &mut buf) as usize;
            let title = String::from_utf16_lossy(&buf[..n.min(512)]);
            let (mut name, mut exe) = (String::new(), String::new());
            if let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut path = [0u16; 1024];
                let mut len = path.len() as u32;
                if QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut len)
                    .is_ok()
                {
                    let full = String::from_utf16_lossy(&path[..len as usize]);
                    name = std::path::Path::new(&full)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    exe = full;
                }
                let _ = CloseHandle(p);
            }
            (name, title, exe)
        }
    }

    pub fn focus_editable() -> Option<bool> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let auto: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let el = auto.GetFocusedElement().ok()?;
            if let Ok(p) = el.GetCurrentPattern(UIA_ValuePatternId) {
                if let Ok(vp) = p.cast::<IUIAutomationValuePattern>() {
                    if let Ok(ro) = vp.CurrentIsReadOnly() {
                        return Some(!ro.as_bool());
                    }
                }
            }
            let ct = el.CurrentControlType().map(|c| c.0).unwrap_or(0);
            if ct == UIA_EditControlTypeId.0 || ct == UIA_DocumentControlTypeId.0 {
                return Some(true);
            }
            let class = el.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
            const KNOWN: [&str; 10] = [
                "Scintilla",
                "RichEdit",
                "Edit",
                "ConsoleWindowClass",
                "TermControl",
                "CASCADIA",
                "Chrome_RenderWidgetHostHWND",
                "MozillaWindowClass",
                "_WwG",
                "Intermediate D3D Window",
            ];
            if KNOWN.iter().any(|k| class.contains(k)) {
                return Some(true);
            }
            Some(false)
        }
    }

    pub fn browser_host() -> Option<String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let auto: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let h: HWND = GetForegroundWindow();
            if h.0.is_null() {
                return None;
            }
            let root = auto.ElementFromHandle(h).ok()?;
            let cond = auto
                .CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &windows::core::VARIANT::from(UIA_EditControlTypeId.0),
                )
                .ok()?;
            for wait_ms in [0u64, 120, 300] {
                if wait_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(wait_ms));
                }
                let Ok(found) = root.FindAll(TreeScope_Descendants, &cond) else { continue };
                let total = found.Length().unwrap_or(0);
                let n = total.min(40);
                let mut fallback = None;
                for i in 0..n {
                    let Ok(el) = found.GetElement(i) else { continue };
                    let name = el.CurrentName().map(|b| b.to_string().to_lowercase()).unwrap_or_default();
                    let Ok(p) = el.GetCurrentPattern(UIA_ValuePatternId) else { continue };
                    let Ok(vp) = p.cast::<IUIAutomationValuePattern>() else { continue };
                    let Ok(v) = vp.CurrentValue() else { continue };
                    let Some(host) = super::host_of(&v.to_string()) else { continue };
                    if name.contains("address") || name.contains("url") || name.contains("location") {
                        return Some(host);
                    }
                    fallback.get_or_insert(host);
                }
                if fallback.is_some() {
                    return fallback;
                }
                if total > 0 {
                    return None;
                }
            }
            None
        }
    }

    // Something you can type into: a writable value, an edit or document control, or an editor that exposes its text
    // and holds the keyboard focus (rich editors in browsers and Electron apps often show up as a plain group).
    fn typeable(el: &IUIAutomationElement) -> bool {
        unsafe {
            if let Ok(p) = el.GetCurrentPattern(UIA_ValuePatternId) {
                if let Ok(vp) = p.cast::<IUIAutomationValuePattern>() {
                    if let Ok(ro) = vp.CurrentIsReadOnly() {
                        return !ro.as_bool();
                    }
                }
            }
            let ct = el.CurrentControlType().map(|c| c.0).unwrap_or(0);
            if ct == UIA_EditControlTypeId.0 || ct == UIA_DocumentControlTypeId.0 {
                return true;
            }
            el.CurrentHasKeyboardFocus().map(|b| b.as_bool()).unwrap_or(false) && el.GetCurrentPattern(UIA_TextPatternId).is_ok()
        }
    }

    pub fn focus_text_box() -> bool {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(auto) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            else {
                return false;
            };
            // already in a text box: type there, never move the cursor to another one
            if auto.GetFocusedElement().is_ok_and(|f| typeable(&f)) {
                return true;
            }
            let h: HWND = GetForegroundWindow();
            if h.0.is_null() {
                return false;
            }
            let Ok(root) = auto.ElementFromHandle(h) else { return false };
            let (Ok(edit), Ok(doc)) = (
                auto.CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &windows::core::VARIANT::from(UIA_EditControlTypeId.0),
                ),
                auto.CreatePropertyCondition(
                    UIA_ControlTypePropertyId,
                    &windows::core::VARIANT::from(UIA_DocumentControlTypeId.0),
                ),
            ) else {
                return false;
            };
            let Ok(either) = auto.CreateOrCondition(&edit, &doc) else { return false };
            let Ok(found) = root.FindAll(TreeScope_Descendants, &either) else { return false };
            let n = found.Length().unwrap_or(0).min(400);
            let mut best: Option<(IUIAutomationElement, i32, i32)> = None;
            for i in 0..n {
                let Ok(el) = found.GetElement(i) else { continue };
                if !el.CurrentIsKeyboardFocusable().map(|b| b.as_bool()).unwrap_or(false) {
                    continue;
                }
                if el.CurrentIsOffscreen().map(|b| b.as_bool()).unwrap_or(true) {
                    continue;
                }
                if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
                    continue;
                }
                if let Ok(p) = el.GetCurrentPattern(UIA_ValuePatternId) {
                    if let Ok(vp) = p.cast::<IUIAutomationValuePattern>() {
                        if vp.CurrentIsReadOnly().map(|b| b.as_bool()).unwrap_or(true) {
                            continue;
                        }
                    }
                } else if el.CurrentControlType().map(|c| c.0).unwrap_or(0) == UIA_DocumentControlTypeId.0 {
                    continue;
                }
                let Ok(r) = el.CurrentBoundingRectangle() else { continue };
                let (w, hgt) = (r.right - r.left, r.bottom - r.top);
                if w < 120 || hgt < 14 {
                    continue;
                }
                if best.as_ref().map_or(true, |(_, bottom, width)| {
                    r.bottom > *bottom || (r.bottom == *bottom && w > *width)
                }) {
                    best = Some((el, r.bottom, w));
                }
            }
            match best {
                Some((el, _, _)) => el.SetFocus().is_ok(),
                None => false,
            }
        }
    }

    fn before_caret(el: &IUIAutomationElement, max: usize) -> Option<String> {
        unsafe {
            let tp: IUIAutomationTextPattern = el.GetCurrentPattern(UIA_TextPatternId).ok()?.cast().ok()?;
            let sel = tp.GetSelection().ok()?;
            if sel.Length().ok()? < 1 {
                return None;
            }
            let caret = sel.GetElement(0).ok()?;
            let range = tp.DocumentRange().ok()?;
            // only read the last `max` chars before the caret
            range
                .MoveEndpointByRange(TextPatternRangeEndpoint_End, &caret, TextPatternRangeEndpoint_Start)
                .ok()?;
            range
                .MoveEndpointByRange(TextPatternRangeEndpoint_Start, &range, TextPatternRangeEndpoint_End)
                .ok()?;
            range
                .MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -(max as i32))
                .ok()?;
            // browsers let that step walk out of the box and into the page before it
            let field = tp.DocumentRange().ok()?;
            let from = TextPatternRangeEndpoint_Start;
            if range.CompareEndpoints(from, &field, from).ok()? < 0 {
                range.MoveEndpointByRange(from, &field, from).ok()?;
            }
            Some(range.GetText(max as i32 * 2 + 16).ok()?.to_string())
        }
    }

    // plain EDIT controls: caret from EM_GETSEL (16-bit, so <64k chars only)
    fn before_caret_in_edit(el: &IUIAutomationElement, value: &str) -> Option<String> {
        unsafe {
            if !el.CurrentClassName().ok()?.to_string().to_lowercase().contains("edit") {
                return None;
            }
            let hwnd = el.CurrentNativeWindowHandle().ok()?;
            if hwnd.0 as isize == 0 {
                return None;
            }
            let reply = SendMessageW(hwnd, 0x00B0, WPARAM(0), LPARAM(0)).0 as usize; // EM_GETSEL
            let (start, end) = (reply & 0xFFFF, (reply >> 16) & 0xFFFF);
            let units: Vec<u16> = value.encode_utf16().collect();
            if units.len() >= 0xFFFF || start > units.len() || end > units.len() {
                return None;
            }
            Some(String::from_utf16_lossy(&units[..start]))
        }
    }

    fn whole_value(el: &IUIAutomationElement) -> Option<String> {
        unsafe {
            let vp: IUIAutomationValuePattern = el.GetCurrentPattern(UIA_ValuePatternId).ok()?.cast().ok()?;
            Some(vp.CurrentValue().ok()?.to_string())
        }
    }

    enum Kind {
        Input,
        NotAnInput,
        Unknown,
    }

    // editable only; a page document reports its URL. Unclear -> unknown (tree may still be loading)
    fn kind_of(el: &IUIAutomationElement) -> Kind {
        unsafe {
            let ct = el.CurrentControlType().map(|c| c.0).unwrap_or(0);
            let writable = el
                .GetCurrentPattern(UIA_ValuePatternId)
                .ok()
                .and_then(|p| p.cast::<IUIAutomationValuePattern>().ok())
                .and_then(|v| v.CurrentIsReadOnly().ok())
                .map(|ro| !ro.as_bool());
            if ct == UIA_EditControlTypeId.0 || ct == UIA_DocumentControlTypeId.0 {
                return if writable == Some(false) { Kind::NotAnInput } else { Kind::Input };
            }
            match writable {
                Some(true) => Kind::Input,
                Some(false) => Kind::NotAnInput,
                None => Kind::Unknown,
            }
        }
    }

    enum Read {
        Text(String),
        Skip,
        TryAgain,
    }

    fn read_element(el: &IUIAutomationElement, max: usize) -> Read {
        unsafe {
            if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
                return Read::Skip;
            }
            match kind_of(el) {
                Kind::NotAnInput => Read::Skip,
                Kind::Unknown => Read::TryAgain,
                Kind::Input => {
                    let value = whole_value(el);
                    if let Some(text) = before_caret(el, max) {
                        // where the box reports its own value, anything outside that value is not the box's text
                        let outside = value.as_deref().is_some_and(|v| !super::is_inside(&text, v));
                        return if outside { Read::Skip } else { Read::Text(text) };
                    }
                    match value {
                        Some(value) => Read::Text(before_caret_in_edit(el, &value).unwrap_or(value)),
                        None => Read::TryAgain,
                    }
                }
            }
        }
    }

    /// Chromium builds its a11y tree lazily, so retry a few times.
    pub fn read_before_cursor(max: usize) -> Option<String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let auto: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            for attempt in 0..4 {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
                let Ok(el) = auto.GetFocusedElement() else { continue };
                match read_element(&el, max) {
                    Read::Text(text) => return super::tidy_context(&text, max),
                    Read::Skip => return None,
                    Read::TryAgain => {}
                }
            }
            None
        }
    }

    // test helper: first text box in a window, by title
    #[cfg(all(test, local_tests))]
    pub fn read_window_for_test(title: &str, max: usize) -> String {
        unsafe fn first_input(
            walker: &IUIAutomationTreeWalker,
            el: &IUIAutomationElement,
            depth: u32,
        ) -> Option<IUIAutomationElement> {
            let mut child = walker.GetFirstChildElement(el).ok();
            while let Some(c) = child {
                let ct = c.CurrentControlType().map(|t| t.0).unwrap_or(0);
                if ct == UIA_EditControlTypeId.0 || ct == UIA_DocumentControlTypeId.0 {
                    return Some(c);
                }
                if depth > 0 {
                    if let Some(f) = first_input(walker, &c, depth - 1) {
                        return Some(f);
                    }
                }
                child = walker.GetNextSiblingElement(&c).ok();
            }
            None
        }
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(auto) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            else {
                return "no automation".into();
            };
            let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
            let hwnd = FindWindowW(windows::core::PCWSTR::null(), windows::core::PCWSTR(wide.as_ptr()))
                .unwrap_or_default();
            if hwnd.0 as isize == 0 {
                return "no such window".into();
            }
            let (Ok(root), Ok(walker)) = (auto.ElementFromHandle(hwnd), auto.ControlViewWalker()) else {
                return "no element".into();
            };
            let Some(el) = first_input(&walker, &root, 4) else { return "no text box in the window".into() };
            match read_element(&el, max) {
                Read::Text(t) => match super::tidy_context(&t, max) {
                    Some(t) => format!(
                        "{} chars, ends with {:?}",
                        t.chars().count(),
                        t.chars().rev().take(40).collect::<String>().chars().rev().collect::<String>()
                    ),
                    None => "nothing written yet".into(),
                },
                Read::Skip => "skipped (a password field or not a text box)".into(),
                Read::TryAgain => "could not read it".into(),
            }
        }
    }

    /// The on-screen rectangles (physical px: x, y, width, height, one per line) a text range covers.
    unsafe fn rects_of(r: &IUIAutomationTextRange) -> Vec<[f64; 4]> {
        use windows::Win32::System::Ole::{SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetUBound, SafeArrayUnaccessData};
        let Ok(sa) = r.GetBoundingRectangles() else { return vec![] };
        if sa.is_null() {
            return vec![];
        }
        let mut out = vec![];
        let n = SafeArrayGetUBound(sa, 1).map(|u| u + 1).unwrap_or(0).max(0) as usize;
        let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
        if n >= 4 && SafeArrayAccessData(sa, &mut data).is_ok() {
            let v = std::slice::from_raw_parts(data as *const f64, n);
            out = v.chunks_exact(4).filter(|c| c[2] > 1.0 && c[3] > 1.0).map(|c| [c[0], c[1], c[2], c[3]]).collect();
            let _ = SafeArrayUnaccessData(sa);
        }
        let _ = SafeArrayDestroy(sa);
        out
    }

    /// Splits `outer` into paragraphs, each with its text and where its lines are on screen. Paragraphs that are off
    /// screen or empty are left out.
    unsafe fn paragraphs(outer: &IUIAutomationTextRange, cap: usize) -> Vec<serde_json::Value> {
        let mut out = vec![];
        let Ok(cur) = outer.Clone() else { return out };
        let _ = cur.MoveEndpointByRange(TextPatternRangeEndpoint_End, outer, TextPatternRangeEndpoint_Start);
        for _ in 0..cap * 3 {
            if out.len() >= cap {
                break;
            }
            let Ok(p) = cur.Clone() else { break };
            if p.ExpandToEnclosingUnit(TextUnit_Paragraph).is_err() {
                break;
            }
            if p.CompareEndpoints(TextPatternRangeEndpoint_Start, outer, TextPatternRangeEndpoint_Start).unwrap_or(0) < 0 {
                let _ = p.MoveEndpointByRange(TextPatternRangeEndpoint_Start, outer, TextPatternRangeEndpoint_Start);
            }
            if p.CompareEndpoints(TextPatternRangeEndpoint_End, outer, TextPatternRangeEndpoint_End).unwrap_or(0) > 0 {
                let _ = p.MoveEndpointByRange(TextPatternRangeEndpoint_End, outer, TextPatternRangeEndpoint_End);
            }
            let text = p.GetText(4000).map(|b| b.to_string()).unwrap_or_default();
            let rects = rects_of(&p);
            if !text.trim().is_empty() && !rects.is_empty() {
                out.push(serde_json::json!({ "text": text.trim(), "rects": rects }));
            }
            if cur.Move(TextUnit_Paragraph, 1).unwrap_or(0) == 0 {
                break;
            }
            if cur.CompareEndpoints(TextPatternRangeEndpoint_Start, outer, TextPatternRangeEndpoint_End).unwrap_or(1) >= 0 {
                break;
            }
        }
        out
    }

    /// The text pattern holding the selection or page: on the focused element or one of its parents, or else the
    /// first document in the window in front.
    unsafe fn text_pattern(auto: &IUIAutomation) -> Vec<IUIAutomationTextPattern> {
        let mut found = vec![];
        let as_tp = |el: &IUIAutomationElement| {
            el.GetCurrentPattern(UIA_TextPatternId).ok().and_then(|p| p.cast::<IUIAutomationTextPattern>().ok())
        };
        if let (Ok(mut el), Ok(walker)) = (auto.GetFocusedElement(), auto.ControlViewWalker()) {
            for _ in 0..12 {
                if let Some(tp) = as_tp(&el) {
                    found.push(tp);
                }
                match walker.GetParentElement(&el) {
                    Ok(p) => el = p,
                    Err(_) => break,
                }
            }
        }
        if let Ok(top) = auto.ElementFromHandle(GetForegroundWindow()) {
            let doc = windows::core::VARIANT::from(UIA_DocumentControlTypeId.0);
            if let Ok(cond) = auto.CreatePropertyCondition(UIA_ControlTypePropertyId, &doc) {
                if let Some(tp) = top.FindFirst(TreeScope_Descendants, &cond).ok().as_ref().and_then(as_tp) {
                    found.push(tp);
                }
            }
        }
        found
    }

    /// The selected text, paragraph by paragraph with where each sits on screen. With `page`, and nothing selected,
    /// the paragraphs visible in the window instead. The bool says whether it is the selection.
    pub fn text_regions(page: bool) -> Option<(Vec<serde_json::Value>, bool)> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let auto: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let patterns = text_pattern(&auto);
            for tp in &patterns {
                let Ok(sel) = tp.GetSelection() else { continue };
                for i in 0..sel.Length().unwrap_or(0) {
                    let Ok(r) = sel.GetElement(i) else { continue };
                    if r.GetText(1).map(|b| b.is_empty()).unwrap_or(true) {
                        continue;
                    }
                    let found = paragraphs(&r, 60);
                    if !found.is_empty() {
                        return Some((found, true));
                    }
                }
            }
            if !page {
                return None;
            }
            for tp in &patterns {
                let Ok(vis) = tp.GetVisibleRanges() else { continue };
                let mut found = vec![];
                for i in 0..vis.Length().unwrap_or(0) {
                    if let Ok(r) = vis.GetElement(i) {
                        found.extend(paragraphs(&r, 80 - found.len()));
                    }
                }
                if !found.is_empty() {
                    return Some((found, false));
                }
            }
            None
        }
    }

    pub fn read_focus() -> Option<(String, String)> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let auto: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let el = auto.GetFocusedElement().ok()?;
            if el.CurrentIsPassword().ok()?.as_bool() {
                return None;
            }
            let key = format!(
                "{}:{}:{}:{}:{}",
                el.CurrentProcessId().unwrap_or(0),
                el.CurrentNativeWindowHandle().map(|h| h.0 as isize).unwrap_or(0),
                el.CurrentAutomationId().unwrap_or_default(),
                el.CurrentClassName().unwrap_or_default(),
                el.CurrentControlType().unwrap_or_default().0
            );
            if let Ok(p) = el.GetCurrentPattern(UIA_ValuePatternId) {
                if let Ok(vp) = p.cast::<IUIAutomationValuePattern>() {
                    let v: BSTR = vp.CurrentValue().ok()?;
                    return Some((v.to_string(), key));
                }
            }
            if let Ok(p) = el.GetCurrentPattern(UIA_TextPatternId) {
                if let Ok(tp) = p.cast::<IUIAutomationTextPattern>() {
                    let t: BSTR = tp.DocumentRange().ok()?.GetText(100000).ok()?;
                    return Some((t.to_string(), key));
                }
            }
            None
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn down(_: i32) -> bool {
        false
    }
    pub fn paste() {}
    pub fn copy() {}
    pub fn undo() {}
    pub fn backspace(_: usize) {}
    pub fn mask_start_menu() {}
    pub fn note_win(_: bool) {}
    pub fn pressed() -> Vec<i32> {
        vec![]
    }
    pub fn private_memory_mb(_pid: u32) -> f64 {
        0.0
    }
    pub fn raise_under_taskbar(_overlay: isize) {}
    pub fn trim_working_set(_pid: u32) -> bool {
        false
    }
    pub fn foreground_full() -> (String, String, String) {
        (String::new(), String::new(), String::new())
    }
    pub fn focus_editable() -> Option<bool> {
        None
    }
    pub fn focus_text_box() -> bool {
        false
    }
    pub fn browser_host() -> Option<String> {
        None
    }
    pub fn read_focus() -> Option<(String, String)> {
        None
    }
    pub fn text_regions(_page: bool) -> Option<(Vec<serde_json::Value>, bool)> {
        None
    }
    pub fn read_before_cursor(_max: usize) -> Option<String> {
        None
    }
}

pub fn start_hotkeys(emit: impl Fn(Value) + Send + 'static) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::Builder::new()
            .name("hotkeys".into())
            .spawn(move || {
                let mut deb = Debounce::new();
                loop {
                    let hit = {
                        let c = COMBOS.lock().unwrap();
                        if c.is_empty() {
                            None
                        } else {
                            pick(&c, &win::down)
                        }
                    };
                    for e in deb.step(hit) {
                        match e {
                            Edge::Up(n) => emit(json!({ "ev": "hk", "name": n, "state": "up" })),
                            Edge::Down(n) => {
                                let uses_win =
                                    COMBOS.lock().unwrap().get(&n).is_some_and(|k| k.contains(&91));
                                if uses_win {
                                    win::mask_start_menu();
                                }
                                emit(json!({ "ev": "hk", "name": n, "state": "down" }));
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(if COMBOS.lock().unwrap().is_empty() {
                        250
                    } else {
                        15
                    }));
                }
            })
            .ok();
    });
}

pub async fn handle(op: &str, req: &Value) -> Option<Value> {
    let id = req["id"].clone();
    Some(match op {
        "native_hello" => json!({ "id": id, "ok": cfg!(windows) }),
        "native_combos" => {
            set_combos(&req["combos"]);
            json!({ "id": id, "ok": true })
        }
        "native_fg" => {
            let (process, title, _) =
                tokio::task::spawn_blocking(win::foreground_full).await.unwrap_or_default();
            json!({ "id": id, "ok": true, "process": process, "title": title })
        }
        "native_paste" => {
            tokio::task::spawn_blocking(win::paste).await.ok();
            json!({ "id": id, "ok": true })
        }
        "native_copy" => {
            tokio::task::spawn_blocking(win::copy).await.ok();
            json!({ "id": id, "ok": true })
        }
        "native_undo" => {
            tokio::task::spawn_blocking(win::undo).await.ok();
            json!({ "id": id, "ok": true })
        }
        "native_down" => {
            let keys = tokio::task::spawn_blocking(win::pressed).await.unwrap_or_default();
            win::note_win(keys.contains(&91));
            json!({ "id": id, "ok": true, "keys": keys })
        }
        "native_read" => {
            let job = tokio::task::spawn_blocking(win::read_focus);
            match tokio::time::timeout(Duration::from_millis(2500), job).await {
                Ok(Ok(Some((text, key)))) => json!({ "id": id, "ok": true, "text": text, "key": key }),
                _ => json!({ "id": id, "ok": false }),
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combos() -> Combos {
        let mut m = HashMap::new();
        m.insert("dictate".to_string(), vec![0x11, 91]);
        m.insert("handsFree".to_string(), vec![0x11, 0x12, 0x44]);
        m.insert("big".to_string(), vec![0x11, 0x12, 0x10, 91, 0x4B, 0x39]);
        m.insert("empty".to_string(), vec![]);
        m
    }

    #[test]
    fn longest_matching_shortcut_wins_and_any_number_of_keys_work() {
        let c = combos();
        let held = |keys: &'static [i32]| move |k: i32| keys.contains(&k);
        assert_eq!(pick(&c, &held(&[0x11, 91])), Some("dictate".into()));
        assert_eq!(pick(&c, &held(&[0x11, 0x12, 0x44])), Some("handsFree".into()));
        assert_eq!(
            pick(&c, &held(&[0x11, 0x12, 0x10, 91, 0x4B, 0x39])),
            Some("big".into()),
            "six keys, and it beats the two key shortcut it contains"
        );
        assert_eq!(pick(&c, &held(&[0x12])), None);
        assert_eq!(pick(&HashMap::new(), &held(&[0x11])), None);
    }

    #[test]
    fn a_brief_flicker_is_not_a_release() {
        let mut d = Debounce::new();
        assert!(matches!(d.step(Some("dictate".into())).as_slice(), [Edge::Down(n)] if n == "dictate"));
        for _ in 0..7 {
            assert!(d.step(None).is_empty(), "7 misses in a row: still held");
        }
        assert!(d.step(Some("dictate".into())).is_empty(), "it came back: nothing happened");
        for _ in 0..7 {
            assert!(d.step(None).is_empty());
        }
        assert!(matches!(d.step(None).as_slice(), [Edge::Up(n)] if n == "dictate"), "8th miss: released");
    }

    #[test]
    fn switching_shortcut_releases_the_old_one() {
        let mut d = Debounce::new();
        d.step(Some("a".into()));
        let e = d.step(Some("b".into()));
        assert!(matches!(e.as_slice(), [Edge::Up(a), Edge::Down(b)] if a == "a" && b == "b"));
    }
}

fn vk_of(name: &str) -> Option<i32> {
    let n = name.trim().to_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" => 0x11,
        "alt" => 0x12,
        "shift" => 0x10,
        "win" | "windows" | "meta" | "super" => 91,
        "space" => 0x20,
        "escape" | "esc" => 0x1b,
        "enter" | "return" => 0x0d,
        "tab" => 0x09,
        "backspace" => 0x08,
        "capslock" => 0x14,
        "insert" => 0x2d,
        "delete" => 0x2e,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        ";" => 0xba,
        "=" => 0xbb,
        "," => 0xbc,
        "-" => 0xbd,
        "." => 0xbe,
        "/" => 0xbf,
        "`" => 0xc0,
        "[" => 0xdb,
        "\\" => 0xdc,
        "]" => 0xdd,
        "'" => 0xde,
        _ => {
            if n.len() >= 2 && n.starts_with('f') {
                if let Ok(i) = n[1..].parse::<i32>() {
                    if (1..=24).contains(&i) {
                        return Some(0x6f + i);
                    }
                }
            }
            let c = n.chars().next()?;
            if n.chars().count() == 1 && c.is_ascii_digit() {
                return Some(0x30 + (c as i32 - '0' as i32));
            }
            if n.chars().count() == 1 && c.is_ascii_lowercase() {
                return Some(c as i32 - 'a' as i32 + 65);
            }
            return None;
        }
    })
}

pub fn parse_combo(s: &str) -> Vec<i32> {
    let mut out: Vec<i32> = vec![];
    for part in s.split('+').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        match vk_of(part) {
            Some(c) => {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
            None => return vec![],
        }
    }
    out
}

#[cfg(test)]
mod name_tests {
    use super::*;
    #[test]
    fn web_addresses_become_host_names() {
        assert_eq!(host_of("https://mail.google.com/mail/u/0/#inbox").as_deref(), Some("mail.google.com"));
        assert_eq!(host_of("www.youtube.com/watch?v=1").as_deref(), Some("youtube.com"));
        assert_eq!(host_of("chatgpt.com").as_deref(), Some("chatgpt.com"));
        for no in ["chrome://settings", "search for cats", "", "localhost:3000", "about:blank"] {
            assert_eq!(host_of(no), None, "{no}");
        }
    }
    #[test]
    fn shortcuts_parse_to_key_codes_with_any_number_of_keys() {
        assert_eq!(parse_combo("Ctrl+Win"), vec![0x11, 91]);
        assert_eq!(parse_combo("Ctrl+Alt+Shift+Win+K+9").len(), 6);
        assert_eq!(parse_combo("Ctrl+Alt+Shift+Win+F9+Space+Z").len(), 7);
        assert_eq!(parse_combo("Escape"), vec![0x1b]);
        assert_eq!(parse_combo("Ctrl+Nonsense"), Vec::<i32>::new());
        assert_eq!(parse_combo(""), Vec::<i32>::new());
        assert_eq!(parse_combo("ctrl+ctrl+a"), vec![0x11, 65]);
    }
}

pub fn name_of_vk(code: i32) -> Option<String> {
    Some(match code {
        0x11 => "Ctrl".into(),
        0x12 => "Alt".into(),
        0x10 => "Shift".into(),
        91 => "Win".into(),
        0x20 => "Space".into(),
        0x1b => "Escape".into(),
        0x0d => "Enter".into(),
        0x09 => "Tab".into(),
        0x08 => "Backspace".into(),
        0x14 => "CapsLock".into(),
        0x2d => "Insert".into(),
        0x2e => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0xba => ";".into(),
        0xbb => "=".into(),
        0xbc => ",".into(),
        0xbd => "-".into(),
        0xbe => ".".into(),
        0xbf => "/".into(),
        0xc0 => "`".into(),
        0xdb => "[".into(),
        0xdc => "\\".into(),
        0xdd => "]".into(),
        0xde => "'".into(),
        0x70..=0x87 => format!("F{}", code - 0x6f),
        0x30..=0x39 => ((b'0' + (code - 0x30) as u8) as char).to_string(),
        65..=90 => ((code as u8) as char).to_string(),
        _ => return None,
    })
}

pub fn foreground_full() -> (String, String, String) {
    win::foreground_full()
}
pub fn focus_editable() -> Option<bool> {
    win::focus_editable()
}
pub const BROWSERS: &[&str] = &[
    "chrome",
    "msedge",
    "brave",
    "firefox",
    "opera",
    "opera_gx",
    "vivaldi",
    "arc",
    "zen",
    "librewolf",
    "waterfox",
    "chromium",
    "thorium",
    "floorp",
    "yandex",
];

pub fn host_of(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return None;
    }
    if t.contains("://") && !(t.starts_with("http://") || t.starts_with("https://")) {
        return None;
    }
    let rest = t.split("://").nth(1).unwrap_or(t);
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?.to_lowercase();
    let labels_ok =
        host.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let tld_ok = host
        .rsplit('.')
        .next()
        .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()));
    if host.contains('.') && labels_ok && tld_ok {
        Some(host.trim_start_matches("www.").to_string())
    } else {
        None
    }
}

pub fn browser_host() -> Option<String> {
    win::browser_host()
}

pub fn raise_under_taskbar(overlay: isize) {
    win::raise_under_taskbar(overlay)
}
pub fn trim_working_set(pid: u32) -> bool {
    win::trim_working_set(pid)
}

pub fn private_memory_mb(pid: u32) -> f64 {
    win::private_memory_mb(pid)
}

static SEEN: LazyLock<Mutex<Option<(String, String, Option<String>)>>> = LazyLock::new(|| Mutex::new(None));

pub fn start_site_watch(on_site: impl Fn(String) + Send + 'static) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::Builder::new()
            .name("site-watch".into())
            .spawn(move || {
                let mut last_site: Option<String> = None;
                loop {
                    std::thread::sleep(Duration::from_millis(450));
                    let (name, title, exe) = foreground_full();
                    if !BROWSERS.contains(&name.to_lowercase().as_str()) {
                        continue;
                    }
                    let same =
                        SEEN.lock().unwrap().as_ref().is_some_and(|(e, t, _)| *e == exe && *t == title);
                    if same {
                        continue;
                    }
                    let site = browser_host();
                    *SEEN.lock().unwrap() = Some((exe, title, site.clone()));
                    if let Some(s) = site {
                        if last_site.as_deref() != Some(s.as_str()) {
                            last_site = Some(s.clone());
                            on_site(s);
                        }
                    }
                }
            })
            .ok();
    });
}

pub fn site_for(exe: &str, title: &str) -> Option<String> {
    if let Some((e, t, s)) = SEEN.lock().unwrap().clone() {
        if e == exe && t == title && s.is_some() {
            return s;
        }
    }
    let s = browser_host();
    *SEEN.lock().unwrap() = Some((exe.to_string(), title.to_string(), s.clone()));
    s
}

pub fn focus_text_box() -> bool {
    win::focus_text_box()
}

#[cfg(all(test, windows, local_tests))]
pub fn read_window_for_test(title: &str, max: usize) -> String {
    win::read_window_for_test(title, max)
}

/// Stops Windows opening the Start menu when Win is let go after a combination.
pub fn mask_start_menu() {
    win::mask_start_menu()
}

/// The keys held down right now (virtual-key codes).
pub fn keys_down() -> Vec<i32> {
    win::pressed()
}

/// The selected text (or, with `page`, the visible text) paragraph by paragraph, with its place on screen.
pub fn text_regions(page: bool) -> Option<(Vec<Value>, bool)> {
    win::text_regions(page)
}

/// Presses Backspace `n` times in the focused app.
pub fn backspace(n: usize) {
    win::backspace(n)
}

/// Up to `max` chars before the caret in the focused box. None for password fields etc.
pub fn read_before_cursor(max: usize) -> Option<String> {
    #[cfg(test)]
    if let Some(text) = TEST_CONTEXT.lock().unwrap().clone() {
        return tidy_context(&text, max);
    }
    win::read_before_cursor(max)
}

#[cfg(test)]
pub static TEST_CONTEXT: Mutex<Option<String>> = Mutex::new(None);

/// Whether `part` is some of `whole`, ignoring spacing and punctuation, which apps report differently.
fn is_inside(part: &str, whole: &str) -> bool {
    let bare = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    bare(whole).contains(&bare(part))
}

pub fn tidy_context(text: &str, max: usize) -> Option<String> {
    // U+FFFC stands for an image or icon; only the text after the last one is known to be next to the cursor
    let text = text.rsplit('\u{fffc}').next().unwrap_or(text);
    let mut clean: String = text.chars().filter(|c| !c.is_control() || *c == '\n').collect();
    while clean.contains("\n\n\n") {
        clean = clean.replace("\n\n\n", "\n\n");
    }
    let skip = clean.chars().count().saturating_sub(max);
    // trailing spaces are kept: box joining needs to know exactly what sits before the cursor
    let tail: String = clean.chars().skip(skip).collect();
    if tail.trim().is_empty() {
        None
    } else {
        Some(tail.to_string())
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;
    #[test]
    fn context_keeps_the_end_drops_control_characters_and_says_none_when_empty() {
        assert_eq!(tidy_context("Hello world, this is ", 800).as_deref(), Some("Hello world, this is "));
        assert_eq!(tidy_context("a\u{0}b\u{7}c", 800).as_deref(), Some("abc"));
        assert_eq!(tidy_context("one two three four", 9).as_deref(), Some("hree four"));
        assert_eq!(tidy_context("line one\n\n\n\nline two", 800).as_deref(), Some("line one\n\nline two"));
        assert_eq!(tidy_context("   \t ", 800), None);
        assert_eq!(tidy_context("", 800), None);
        assert_eq!(tidy_context("héllo wörld ñ", 5).as_deref(), Some("rld ñ"));
    }

    #[test]
    fn page_text_around_the_box_is_not_context() {
        assert!(!is_inside("Accessibility help\ni\n", ""));
        assert!(!is_inside("Accessibility help\nHello there. ", "Hello there. "));
        assert!(is_inside("Hello there.\r\nSecond", "Hello there.\nSecond line"));
        assert!(is_inside("", "anything"));
        assert_eq!(tidy_context("Accessibility help \u{fffc} \u{fffc}", 800), None);
        assert_eq!(tidy_context("help \u{fffc}Hello", 800).as_deref(), Some("Hello"));
    }
}
