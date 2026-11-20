use crate::store::Store;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

pub struct Core {
    pub store: Mutex<Store>,
    pub version: Mutex<String>,
    pub helper_ready: Mutex<bool>,
}

static CORE: OnceLock<Core> = OnceLock::new();

pub fn init(dir: &std::path::Path, version: &str) -> &'static Core {
    CORE.get_or_init(|| Core {
        store: Mutex::new(Store::open(dir)),
        version: Mutex::new(version.to_string()),
        helper_ready: Mutex::new(false),
    })
}
pub fn get() -> &'static Core {
    CORE.get().expect("core not initialised: send the init op first")
}
pub fn try_get() -> Option<&'static Core> {
    CORE.get()
}

pub fn push(ch: &str, to: &str, payload: Value) {
    crate::say(json!({ "ev": "push", "ch": ch, "to": to, "payload": payload }));
}

pub fn state_for_renderer() -> Value {
    let c = get();
    let (cfg, status, history) = {
        let s = c.store.lock().unwrap();
        let mut status = serde_json::Map::new();
        for p in s.providers() {
            let id = p["id"].as_str().unwrap_or("");
            status
                .insert(id.to_string(), json!({ "hasKey": s.has_secret(id), "ready": s.provider_ready(id) }));
        }
        (s.config.clone(), Value::Object(status), s.history.iter().take(300).cloned().collect::<Vec<_>>())
    };
    json!({
        "config": cfg, "providerStatus": status, "suggestions": [], "history": history,
        "session": crate::session::summary(),
        "helperReady": *c.helper_ready.lock().unwrap(),
        "version": *c.version.lock().unwrap(),
    })
}

pub fn changed(what: &str) {
    push("state:changed", "all", state_for_renderer());
    if what == "config" {
        push("shell:apply", "shell", Value::Null);
        apply_hotkeys();
    }
    if what == "usage" {
        push("usage:changed", "main", Value::Null);
    }
}

pub fn apply_hotkeys() {
    let c = get();
    let h = c.store.lock().unwrap().config["hotkeys"].clone();
    let mut combos = serde_json::Map::new();
    for name in ["dictate", "handsFree", "cancel", "undo", "formatSelection"] {
        combos.insert(name.to_string(), json!(crate::native::parse_combo(h[name].as_str().unwrap_or(""))));
    }
    crate::native::set_combos(&Value::Object(combos));
}
