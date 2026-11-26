use tao::dpi::LogicalSize;
use tao::event_loop::{EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Window, WindowBuilder};
use wry::{WebContext, WebView, WebViewBuilder};

pub const NO_WINDOW: u32 = 0x0800_0000;

pub enum Ev {
    Ipc(String),
    Js(String),
}

pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

pub struct Shell {
    pub event_loop: EventLoop<Ev>,
    pub proxy: EventLoopProxy<Ev>,
    pub window: Window,
    pub webview: Result<WebView, wry::Error>,
}

pub fn open(title: &str, html: &'static str, ctx: &mut WebContext) -> Shell {
    let event_loop = EventLoopBuilder::<Ev>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(880.0, 560.0))
        .with_resizable(false)
        .with_decorations(false)
        .with_undecorated_shadow(true)
        .with_visible(false)
        .build(&event_loop)
        .expect("window");
    round_corners(window.hwnd());
    if let Some(m) = window.current_monitor() {
        let (ms, mp, ws) = (m.size(), m.position(), window.outer_size());
        window.set_outer_position(tao::dpi::PhysicalPosition::new(
            mp.x + (ms.width as i32 - ws.width as i32) / 2,
            mp.y + (ms.height as i32 - ws.height as i32) / 2,
        ));
    }
    let ipc_proxy = proxy.clone();
    let webview = WebViewBuilder::new_with_web_context(ctx)
        // too big for with_html
        .with_custom_protocol("setup".into(), move |_id, _req| {
            wry::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(std::borrow::Cow::Borrowed(html.as_bytes()))
                .unwrap()
        })
        .with_url("http://setup.localhost/")
        .with_background_color((0, 0, 0, 255))
        .with_devtools(false)
        .with_ipc_handler(move |req| {
            let _ = ipc_proxy.send_event(Ev::Ipc(req.body().clone()));
        })
        .build(&window);
    Shell { event_loop, proxy, window, webview }
}

fn round_corners(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    unsafe {
        let pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            HWND(hwnd as _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as _,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}
