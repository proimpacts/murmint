use base64::Engine as _;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn defaults() -> Value {
    let mut v: Value = serde_json::from_str(include_str!("../data/defaults.json")).expect("defaults");
    v["personal"] = json!({ "vocabulary": [] });
    v
}
fn old_styles() -> Value {
    serde_json::from_str(include_str!("../data/old_styles.json")).unwrap_or(Value::Null)
}
fn old_apps() -> Value {
    serde_json::from_str(include_str!("../data/old_apps.json")).unwrap_or(Value::Null)
}
/// Undo what removed extensions left in old configs.
fn drop_extension_leftovers(cfg: &mut Value) {
    let installed = cfg["extensions"]["installed"].as_array().cloned().unwrap_or_default();
    for rec in &installed {
        if let Some(prev) = rec["prev"].as_object() {
            for (path, value) in prev {
                set_path(cfg, path, value.clone());
            }
        }
    }
    let from_extension = |x: &Value| x.get("ext").is_some();
    let mut removed_styles = vec![];
    if let Some(styles) = cfg["styles"].as_array_mut() {
        styles.retain(|s| {
            let gone = from_extension(s);
            if gone {
                removed_styles.push(s["id"].clone());
            }
            !gone
        });
    }
    for path in ["formatting", "personal"] {
        let key = if path == "formatting" { "rules" } else { "vocabulary" };
        if let Some(list) = cfg[path][key].as_array_mut() {
            list.retain(|x| !from_extension(x));
        }
    }
    if removed_styles.contains(&cfg["formatting"]["style"]) {
        cfg["formatting"]["style"] = json!("natural");
    }
    if let Some(profiles) = cfg["appProfiles"].as_array_mut() {
        for p in profiles.iter_mut() {
            if from_extension(p) {
                if let Some(prev) = p.get("extPrev").cloned() {
                    p["style"] = prev;
                }
            } else if removed_styles.contains(&p["style"]) {
                p["style"] = json!("natural");
            }
            if let Some(o) = p.as_object_mut() {
                o.remove("ext");
                o.remove("extPrev");
            }
        }
    }
    if let Some(c) = cfg.as_object_mut() {
        c.remove("extensions");
    }
    if cfg["insertion"]["method"] == "type" {
        cfg["insertion"]["method"] = json!("paste");
    }
    if let Some(i) = cfg["insertion"].as_object_mut() {
        i.remove("typeDelayMs");
    }
}

fn is_old_text(old: &Value, cur: &Value) -> bool {
    match old {
        Value::Array(a) => a.iter().any(|x| x == cur),
        Value::Null => false,
        v => v == cur,
    }
}

fn is_obj(v: &Value) -> bool {
    v.is_object()
}

pub fn deep_merge(base: &Value, over: &Value) -> Value {
    if !is_obj(base) || !is_obj(over) {
        return if over.is_null() { base.clone() } else { over.clone() };
    }
    let mut out = base.as_object().unwrap().clone();
    for (k, v) in over.as_object().unwrap() {
        let merged = match out.get(k) {
            Some(b) if is_obj(b) && is_obj(v) => deep_merge(b, v),
            _ => v.clone(),
        };
        out.insert(k.clone(), merged);
    }
    Value::Object(out)
}

pub fn set_path(root: &mut Value, dotted: &str, value: Value) {
    let parts: Vec<&str> = dotted.split('.').collect();
    if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
        return;
    }
    let mut cur = root;
    for p in &parts[..parts.len() - 1] {
        match cur {
            Value::Array(arr) => {
                let Ok(idx) = p.parse::<usize>() else { return };
                if idx >= arr.len() {
                    return;
                }
                cur = &mut arr[idx];
            }
            Value::Object(map) => {
                if !map.get(*p).is_some_and(|v| v.is_object() || v.is_array()) {
                    map.insert((*p).to_string(), json!({}));
                }
                cur = map.get_mut(*p).unwrap();
            }
            _ => return,
        }
    }
    let leaf = parts[parts.len() - 1];
    match cur {
        Value::Array(arr) => {
            if let Ok(idx) = leaf.parse::<usize>() {
                if idx < arr.len() {
                    arr[idx] = value;
                }
            }
        }
        Value::Object(map) => {
            map.insert(leaf.to_string(), value);
        }
        _ => {}
    }
}

#[cfg(test)]
pub static FORCE_DPAPI_FAIL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};
    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let inb = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
            let mut out = CRYPT_INTEGER_BLOB::default();
            CryptProtectData(&inb, windows::core::PCWSTR::null(), None, None, None, 0, &mut out).ok()?;
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(HLOCAL(out.pbData as _));
            Some(v)
        }
    }
    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let inb = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
            let mut out = CRYPT_INTEGER_BLOB::default();
            CryptUnprotectData(&inb, None, None, None, None, 0, &mut out).ok()?;
            let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(HLOCAL(out.pbData as _));
            Some(v)
        }
    }
}
#[cfg(not(windows))]
mod dpapi {
    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
}

pub struct Store {
    pub dir: PathBuf,
    pub config: Value,
    pub secrets: Map<String, Value>,
    pub history: Vec<Value>,
    pub usage: Value,
    pub latency: HashMap<String, f64>,
    dirty: [bool; 4],
}

const FILES: [&str; 4] = ["config.json", "secrets.json", "history.json", "usage.json"];

impl Store {
    pub fn open(dir: &Path) -> Store {
        let _ = std::fs::create_dir_all(dir);
        let read = |n: &str| {
            std::fs::read_to_string(dir.join(n)).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok())
        };
        let saved = read("config.json").unwrap_or(json!({}));
        let mut s = Store {
            dir: dir.to_path_buf(),
            config: Value::Null,
            secrets: read("secrets.json").and_then(|v| v.as_object().cloned()).unwrap_or_default(),
            history: read("history.json").and_then(|v| v.as_array().cloned()).unwrap_or_default(),
            usage: read("usage.json").filter(|v| v.get("days").is_some()).unwrap_or(json!({ "days": {} })),
            latency: HashMap::new(),
            dirty: [false; 4],
        };
        s.config = s.load_config(&saved);
        s.prune_history();
        s.drop_transcription_leftovers();
        s
    }

    /// The Transcription feature was removed. Clear what it left behind: its service, its key, its usage rows, any
    /// model choice that pointed at it, and its saved recordings list.
    fn drop_transcription_leftovers(&mut self) {
        const ID: &str = "murmint-transcribe";
        let mine = |r: &Value| r.as_str().is_some_and(|r| r.starts_with(&format!("{ID}::")));
        let mut changed = false;
        if let Some(list) = self.config["providers"].as_array_mut() {
            let before = list.len();
            list.retain(|p| p["id"] != ID);
            changed |= list.len() != before;
        }
        if let Some(list) = self.config["shownModels"].as_array_mut() {
            list.retain(|r| !mine(r));
        }
        for job in ["stt", "format"] {
            let m = &mut self.config["models"][job];
            if mine(&m["primary"]) {
                m["primary"] = json!(if job == "stt" { "murmint-local::whisper-tiny" } else { "" });
                changed = true;
            }
            if let Some(list) = m["fallbacks"].as_array_mut() {
                list.retain(|r| !mine(r));
            }
        }
        if changed {
            self.touch("config.json");
        }
        if self.secrets.remove(ID).is_some() {
            self.touch("secrets.json");
        }
        let mut used = false;
        if let Some(days) = self.usage["days"].as_object_mut() {
            for day in days.values_mut() {
                if let Some(by) = day["byModel"].as_object_mut() {
                    let before = by.len();
                    by.retain(|k, _| !k.starts_with(&format!("{ID}::")));
                    used |= by.len() != before;
                }
            }
        }
        if used {
            self.touch("usage.json");
        }
        let _ = std::fs::remove_file(self.dir.join("transcripts.json"));
    }

    fn load_config(&self, saved: &Value) -> Value {
        let def = defaults();
        let mut cfg = deep_merge(&def, saved);
        cfg["providers"] = reconcile_providers(&def["providers"], &saved["providers"]);
        if let (Some(list), Some(gone)) =
            (cfg["providers"].as_array_mut(), saved["removedProviders"].as_array())
        {
            list.retain(|p| !gone.contains(&p["id"]));
        }
        cfg["styles"] = reconcile_by_id(&def["styles"], &saved["styles"]);
        cfg["appProfiles"] = reconcile_by_id(&def["appProfiles"], &saved["appProfiles"]);
        let old = old_styles();
        if let Some(styles) = cfg["styles"].as_array_mut() {
            for st in styles.iter_mut() {
                let id = st["id"].as_str().unwrap_or("").to_string();
                let ship = def["styles"].as_array().and_then(|a| a.iter().find(|x| x["id"] == id)).cloned();
                if let (Some(ship), true) = (ship, st["builtin"] == json!(true)) {
                    if is_old_text(&old[&id], &st["instruction"]) {
                        st["instruction"] = ship["instruction"].clone();
                        st["description"] = ship["description"].clone();
                    }
                }
            }
        }
        if let Some(ps) = cfg["appProfiles"].as_array_mut() {
            for p in ps.iter_mut() {
                if let Some(ship) =
                    def["appProfiles"].as_array().and_then(|a| a.iter().find(|x| x["id"] == p["id"]))
                {
                    p["process"] = ship["process"].clone();
                    p["title"] = ship["title"].clone();
                    p["name"] = ship["name"].clone();
                    // migrate untouched app notes to the new text
                    if saved["promptsVersion"].as_u64().unwrap_or(1) < 3 {
                        let id = p["id"].as_str().unwrap_or("").to_string();
                        let cur = p["instruction"].as_str().unwrap_or("").to_string();
                        let was = &old_apps()[&id];
                        let shipped_before = match was {
                            Value::Array(a) => a.iter().any(|x| x.as_str() == Some(cur.as_str())),
                            v => v.as_str() == Some(cur.as_str()),
                        };
                        if cur.trim().is_empty() || shipped_before {
                            p["instruction"] = ship["instruction"].clone();
                        }
                    }
                }
            }
        }
        cfg["promptsVersion"] = def["promptsVersion"].clone();
        if cfg["appearance"]["accent"] == "violet" {
            cfg["appearance"]["accent"] = json!("mono");
        }
        if cfg["appearance"]["accent"] == "mono" && cfg["appearance"]["accentPicked"] != json!(true) {
            cfg["appearance"]["accent"] = json!("ember");
        }
        if saved["vocabAutoCleaned"] != json!(true) {
            if let Some(v) = cfg["personal"]["vocabulary"].as_array_mut() {
                v.retain(|e| !matches!(e["note"].as_str(), Some("auto") | Some("edit")));
            }
        }
        cfg["vocabAutoCleaned"] = json!(true);
        if let Some(p) = cfg["personal"].as_object_mut() {
            for k in ["termCounts", "dismissed", "learnEdits", "corrections", "snippets"] {
                p.remove(k);
            }
        }
        let old = saved["formatting"]["customInstruction"].as_str().unwrap_or("").trim().to_string();
        if let Some(f) = cfg["formatting"].as_object_mut() {
            f.remove("customInstruction");
            if !old.is_empty() && f.get("rules").and_then(|r| r.as_array()).map_or(true, |a| a.is_empty()) {
                f.insert(
                    "rules".into(),
                    json!([{ "id": uuid::Uuid::new_v4().to_string(), "text": old, "enabled": true }]),
                );
            }
        }
        drop_extension_leftovers(&mut cfg);
        cfg["dictation"]["streaming"] = json!(false);
        // no longer settings: always on
        cfg["formatting"]["removeFillers"] = json!(true);
        if let Some(o) = cfg.as_object_mut() {
            o.remove("aiOnByDefault");
        }
        // 5.2: dark by default (once); the theme can be changed again afterwards
        if saved["darkByDefault"] != json!(true) {
            cfg["appearance"]["theme"] = json!("dark");
        }
        if let Some(o) = cfg.as_object_mut() {
            o.remove("lightByDefault");
        }
        cfg["darkByDefault"] = json!(true);
        cfg
    }

    pub fn touch(&mut self, name: &str) {
        if let Some(i) = FILES.iter().position(|f| *f == name) {
            self.dirty[i] = true;
        }
    }
    // atomic write; failures retry next tick
    pub fn flush(&mut self) {
        for (i, name) in FILES.iter().enumerate() {
            if !self.dirty[i] {
                continue;
            }
            self.dirty[i] = false;
            let data = match i {
                0 => self.config.to_string(),
                1 => Value::Object(self.secrets.clone()).to_string(),
                2 => Value::Array(self.history.clone()).to_string(),
                _ => self.usage.to_string(),
            };
            let tmp = self.dir.join(format!("{name}.tmp"));
            if std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, self.dir.join(name))).is_err() {
                self.dirty[i] = true;
            }
        }
    }
    pub fn has_unsaved(&self) -> bool {
        self.dirty.iter().any(|d| *d)
    }

    pub fn set(&mut self, dotted: &str, value: Value) {
        set_path(&mut self.config, dotted, value);
        self.touch("config.json");
    }
    pub fn replace_section(&mut self, key: &str, value: Value) {
        self.config[key] = value;
        self.touch("config.json");
    }

    pub fn set_secret(&mut self, id: &str, value: &str) {
        if value.is_empty() {
            self.secrets.remove(id);
        } else {
            let enc = dpapi::protect(value.as_bytes())
                .map(|b| json!({ "enc": "dpapi", "v": base64::engine::general_purpose::STANDARD.encode(b) }))
                .unwrap_or_else(
                    || json!({ "enc": false, "v": base64::engine::general_purpose::STANDARD.encode(value) }),
                );
            self.secrets.insert(id.to_string(), enc);
        }
        self.touch("secrets.json");
    }
    pub fn get_secret(&self, id: &str) -> String {
        let Some(s) = self.secrets.get(id) else { return String::new() };
        let Some(raw) =
            s["v"].as_str().and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
        else {
            return String::new();
        };
        match &s["enc"] {
            Value::String(k) if k == "dpapi" => {
                dpapi::unprotect(&raw).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default()
            }
            Value::Bool(false) => String::from_utf8_lossy(&raw).to_string(),
            _ => String::new(),
        }
    }
    pub fn has_secret(&self, id: &str) -> bool {
        self.secrets.contains_key(id)
    }
    pub fn pending_secrets(&self) -> Vec<(String, String)> {
        self.secrets
            .iter()
            .filter(|(_, v)| v["enc"] == json!(true))
            .filter_map(|(k, v)| v["v"].as_str().map(|s| (k.clone(), s.to_string())))
            .collect()
    }

    pub fn providers(&self) -> &Vec<Value> {
        static EMPTY: Vec<Value> = Vec::new();
        self.config["providers"].as_array().unwrap_or(&EMPTY)
    }
    pub fn provider(&self, id: &str) -> Option<&Value> {
        self.providers().iter().find(|p| p["id"] == id)
    }
    pub fn provider_ready(&self, id: &str) -> bool {
        let Some(p) = self.provider(id) else { return false };
        if p["enabled"] == json!(false) || p["enabled"].is_null() {
            return false;
        }
        if !self.has_secret(id) && p["keyOptional"] != json!(true) {
            return false;
        }
        if p["type"] == "cloudflare" && p["accountId"].as_str().unwrap_or("").is_empty() {
            return false;
        }
        true
    }
    pub fn find_model(&self, r: &str) -> Option<(&Value, &Value)> {
        let i = r.find("::")?;
        let p = self.provider(&r[..i])?;
        let m = p["models"].as_array()?.iter().find(|m| m["id"] == r[i + 2..])?;
        Some((p, m))
    }

    pub fn add_history(&mut self, entry: Value) {
        self.history.insert(0, entry);
        self.history.truncate(2000);
        self.touch("history.json");
    }
    pub fn update_history(&mut self, id: &str, patch: &Value) {
        if let Some(e) = self.history.iter_mut().find(|h| h["id"] == id) {
            if let (Some(eo), Some(po)) = (e.as_object_mut(), patch.as_object()) {
                for (k, v) in po {
                    eo.insert(k.clone(), v.clone());
                }
            }
            self.touch("history.json");
        }
    }
    pub fn delete_history(&mut self, id: &str) {
        self.history.retain(|h| h["id"] != id);
        self.touch("history.json");
    }
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.touch("history.json");
    }
    pub fn prune_history(&mut self) {
        // old "VS Code / Cursor" entries -> VS Code
        let mut renamed = false;
        for e in self.history.iter_mut() {
            if e["app"] == "VS Code / Cursor" {
                e["app"] = json!("Visual Studio Code");
                renamed = true;
            }
        }
        if renamed {
            self.touch("history.json");
        }
        let privacy = &self.config["privacy"];
        let days = privacy["retentionDays"].as_i64().unwrap_or(0);
        if privacy["saveHistory"] == json!(false) {
            self.history.clear();
            return;
        }
        if days == 0 {
            return;
        }
        let cutoff = now_ms() - days * 86_400_000;
        self.history.retain(|h| h["ts"].as_i64().unwrap_or(i64::MAX) >= cutoff);
    }

    pub fn record_usage(&mut self, day: &str, patch: &Value) {
        let n = |k: &str| patch[k].as_f64().unwrap_or(0.0);
        let d = &mut self.usage["days"][day];
        if d.is_null() {
            *d = json!({ "dictations": 0, "words": 0, "audioSec": 0, "cost": 0, "requests": 0, "byModel": {} });
        }
        let add = |d: &mut Value, k: &str, v: f64| {
            let cur = d[k].as_f64().unwrap_or(0.0);
            d[k] = json!(cur + v);
        };
        add(d, "dictations", n("dictations"));
        add(d, "words", n("words"));
        add(d, "audioSec", n("audioSec"));
        if let Some(model) = patch["model"].as_str() {
            let m = &mut d["byModel"][model];
            if m.is_null() {
                *m = json!({ "requests": 0, "cost": 0, "audioSec": 0, "tokensIn": 0, "tokensOut": 0 });
            }
            add(m, "requests", 1.0);
            add(m, "cost", n("cost"));
            add(m, "audioSec", n("modelAudioSec"));
            add(m, "tokensIn", n("tokensIn"));
            add(m, "tokensOut", n("tokensOut"));
            add(d, "requests", 1.0);
            add(d, "cost", n("cost"));
        }
        self.touch("usage.json");
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn reconcile_by_id(shipped: &Value, saved: &Value) -> Value {
    let Some(saved) = saved.as_array() else { return shipped.clone() };
    let mut out: Vec<Value> = shipped
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|s| match saved.iter().find(|x| x["id"] == s["id"]) {
            Some(o) => {
                let mut m = deep_merge_shallow(&s, o);
                m["builtin"] = json!(true);
                m
            }
            None => s,
        })
        .collect();
    for s in saved {
        if s["builtin"] != json!(true) && !out.iter().any(|x| x["id"] == s["id"]) {
            out.push(s.clone());
        }
    }
    Value::Array(out)
}
fn deep_merge_shallow(a: &Value, b: &Value) -> Value {
    let mut o = a.as_object().cloned().unwrap_or_default();
    if let Some(bo) = b.as_object() {
        for (k, v) in bo {
            o.insert(k.clone(), v.clone());
        }
    }
    Value::Object(o)
}

fn reconcile_providers(ship: &Value, saved: &Value) -> Value {
    let Some(saved) = saved.as_array() else { return ship.clone() };
    let mut out: Vec<Value> = vec![];
    for p in ship.as_array().cloned().unwrap_or_default() {
        let Some(s) = saved.iter().find(|x| x["id"] == p["id"]) else {
            out.push(p);
            continue;
        };
        let p_models = p["models"].as_array().cloned().unwrap_or_default();
        let mut models: Vec<Value> = s["models"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                let mut base = json!({ "tier": "decent" });
                if let Some(shipm) = p_models.iter().find(|x| x["id"] == m["id"]) {
                    if m["custom"] != json!(true) {
                        if !shipm["englishOnly"].is_null() {
                            base["englishOnly"] = shipm["englishOnly"].clone();
                        }
                        if !shipm["realtime"].is_null() {
                            base["realtime"] = shipm["realtime"].clone();
                        }
                    }
                }
                let mut m = deep_merge_shallow(&base, &m);
                if let Some(shipm) = p_models.iter().find(|x| x["id"] == m["id"]) {
                    if m["custom"] != json!(true) && !shipm["tier"].is_null() {
                        m["tier"] = shipm["tier"].clone();
                    }
                }
                m
            })
            .collect();
        for m in &p_models {
            if !models.iter().any(|x| x["id"] == m["id"]) {
                models.push(m.clone());
            }
        }
        let mut merged = deep_merge_shallow(&p, s);
        for k in ["keyLabel", "note", "docs"] {
            merged[k] = p[k].clone();
        }
        merged["models"] = Value::Array(models);
        merged["builtin"] = json!(true);
        out.push(merged);
    }
    for s in saved {
        if s["builtin"] != json!(true) && !out.iter().any(|x| x["id"] == s["id"]) {
            let mut c = s.clone();
            c["models"] = Value::Array(
                s["models"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| deep_merge_shallow(&json!({ "tier": "decent" }), &m))
                    .collect(),
            );
            out.push(c);
        }
    }
    Value::Array(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_provider_has_exactly_one_best_model_per_task() {
        let cfg = defaults();
        let tiers = ["worst", "bad", "decent", "good", "best"];
        for p in cfg["providers"].as_array().unwrap() {
            for task in ["chat", "stt"] {
                let models: Vec<&Value> = p["models"].as_array().unwrap().iter().filter(|m| m["task"] == task).collect();
                if models.is_empty() {
                    continue;
                }
                assert!(models.iter().all(|m| tiers.contains(&m["tier"].as_str().unwrap_or(""))), "{} {task}", p["id"]);
                let best = models.iter().filter(|m| m["tier"] == "best").count();
                assert_eq!(best, 1, "{} {task} needs exactly one best", p["id"]);
            }
        }
    }

    #[test]
    fn what_an_old_extension_left_behind_is_cleaned_up() {
        let saved = json!({
            "insertion": { "method": "type", "typeDelayMs": 30, "trailingSpace": true },
            "extensions": { "installed": [{ "id": "typewriter", "prev": { "insertion.method": "clipboard" } }] },
            "styles": [{ "id": "ext_calm_0", "ext": "calm" }],
            "formatting": { "style": "ext_calm_0", "rules": [{ "id": "ext_calm_0", "text": "x", "ext": "calm" }, { "id": "mine", "text": "y" }] },
            "appProfiles": [{ "id": "discord", "style": "ext_calm_0", "ext": "calm", "extPrev": "casual" }]
        });
        let mut cfg = deep_merge(&defaults(), &saved);
        drop_extension_leftovers(&mut cfg);
        assert_eq!(cfg["insertion"]["method"], "clipboard");
        assert!(cfg["insertion"].get("typeDelayMs").is_none());
        assert_eq!(cfg["insertion"]["trailingSpace"], true);
        assert!(cfg.get("extensions").is_none());
        assert!(cfg["styles"].as_array().unwrap().iter().all(|s| s.get("ext").is_none()));
        assert_eq!(cfg["formatting"]["style"], "natural");
        assert_eq!(cfg["formatting"]["rules"].as_array().unwrap().len(), 1);
        let discord = cfg["appProfiles"].as_array().unwrap().iter().find(|p| p["id"] == "discord").unwrap();
        assert_eq!(discord["style"], "casual");
    }
    #[test]
    fn built_in_prompts_move_forward_but_edits_are_kept() {
        let def = defaults();
        let old_s = old_styles();
        let old_a = old_apps();
        let mut saved = def.clone();
        saved["promptsVersion"] = json!(1);
        for st in saved["styles"].as_array_mut().unwrap() {
            let id = st["id"].as_str().unwrap().to_string();
            // the text that shipped last; styles added later have none
            if let Some(prev) = old_s[&id].as_array().and_then(|a| a.last()) {
                st["instruction"] = prev.clone();
            }
        }
        saved["styles"][1]["instruction"] = json!("My own casual style.");
        for p in saved["appProfiles"].as_array_mut().unwrap() {
            let was = old_a[p["id"].as_str().unwrap()].clone();
            p["instruction"] = was;
        }
        let me = saved["appProfiles"].as_array().unwrap().iter().position(|p| p["id"] == "slack").unwrap();
        saved["appProfiles"][me]["instruction"] = json!("Slack: always shout.");
        let dir = tmp();
        std::fs::write(dir.join("config.json"), saved.to_string()).unwrap();
        let s = Store::open(&dir);
        let styles = s.config["styles"].as_array().unwrap();
        assert_eq!(styles[0]["instruction"], def["styles"][0]["instruction"]);
        assert_eq!(styles[1]["instruction"], json!("My own casual style."));
        let apps = s.config["appProfiles"].as_array().unwrap();
        let get = |id: &str| apps.iter().find(|p| p["id"] == id).unwrap()["instruction"].clone();
        assert_eq!(
            get("outlook"),
            def["appProfiles"].as_array().unwrap().iter().find(|p| p["id"] == "outlook").unwrap()
                ["instruction"],
            "an empty note takes the new one"
        );
        assert_eq!(get("slack"), json!("Slack: always shout."), "an edited note is kept");
        assert_eq!(s.config["promptsVersion"], json!(3));
        let mut again = s.config.clone();
        let o = again["appProfiles"].as_array().unwrap().iter().position(|p| p["id"] == "outlook").unwrap();
        again["appProfiles"][o]["instruction"] = json!("");
        std::fs::write(dir.join("config.json"), again.to_string()).unwrap();
        let s2 = Store::open(&dir);
        let apps2 = s2.config["appProfiles"].as_array().unwrap();
        assert_eq!(apps2.iter().find(|p| p["id"] == "outlook").unwrap()["instruction"], json!(""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "murmint-store-{}-{}",
            std::process::id(),
            now_ms() * 1000 + rand_bit()
        ));
        let _ = std::fs::create_dir_all(&d);
        d
    }
    fn rand_bit() -> i64 {
        static N: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    #[test]
    fn what_the_removed_transcription_feature_left_is_cleared() {
        let d = tmp();
        let svc = json!({ "id": "murmint-transcribe", "name": "Murmint 1 Transcription", "baseUrl": "http://127.0.0.1:1/v1", "models": [{ "id": "murmint-1-transcription", "label": "x", "task": "stt" }] });
        let cfg = json!({ "providers": [svc], "shownModels": ["murmint-transcribe::murmint-1-transcription"],
            "models": { "stt": { "primary": "murmint-transcribe::murmint-1-transcription", "fallbacks": [], "routing": "manual" } } });
        std::fs::write(d.join("config.json"), cfg.to_string()).unwrap();
        std::fs::write(d.join("secrets.json"), json!({ "murmint-transcribe": "x" }).to_string()).unwrap();
        std::fs::write(d.join("usage.json"), json!({ "days": { "2026-11-25": { "byModel": { "murmint-transcribe::murmint-1-transcription": {}, "openai::whisper-1": {} } } } }).to_string()).unwrap();
        std::fs::write(d.join("transcripts.json"), "[]").unwrap();
        let s = Store::open(&d);
        assert!(!s.providers().iter().any(|p| p["id"] == "murmint-transcribe"));
        assert_eq!(s.config["shownModels"], json!([]));
        assert_eq!(s.config["models"]["stt"]["primary"], "murmint-local::whisper-tiny");
        assert!(!s.secrets.contains_key("murmint-transcribe"));
        assert_eq!(s.usage["days"]["2026-11-25"]["byModel"].as_object().unwrap().len(), 1);
        assert!(!d.join("transcripts.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_new_install_gets_the_defaults_and_keeps_changes() {
        let d = tmp();
        let mut s = Store::open(&d);
        assert_eq!(s.config["hotkeys"]["dictate"], "Ctrl+Win");
        assert!(s.providers().iter().any(|p| p["id"] == "anthropic"));
        s.set("hotkeys.dictate", json!("Ctrl+Alt+Space"));
        s.flush();
        let s2 = Store::open(&d);
        assert_eq!(s2.config["hotkeys"]["dictate"], "Ctrl+Alt+Space");
        assert_eq!(
            s2.config["hotkeys"]["undo"], "Ctrl+Alt+Z",
            "unchanged settings still come from the defaults"
        );
    }

    #[test]
    fn keys_are_encrypted_on_disk_and_come_back() {
        let d = tmp();
        let mut s = Store::open(&d);
        s.set_secret("openai", "sk-secret-123");
        assert_eq!(s.get_secret("openai"), "sk-secret-123");
        s.flush();
        let raw = std::fs::read_to_string(d.join("secrets.json")).unwrap();
        #[cfg(windows)]
        assert!(!raw.contains("sk-secret-123"));
        assert_eq!(Store::open(&d).get_secret("openai"), "sk-secret-123");
        let mut s3 = Store::open(&d);
        s3.set_secret("openai", "");
        assert!(!s3.has_secret("openai"));
    }

    #[test]
    fn words_added_by_older_versions_on_their_own_are_dropped() {
        let d = tmp();
        std::fs::write(d.join("config.json"), json!({ "personal": { "vocabulary": [
            { "id": "1", "term": "FastAPI", "sounds": [], "note": "auto" }, { "id": "2", "term": "Siobhan", "sounds": [], "note": "edit" },
            { "id": "3", "term": "Kubernetes", "sounds": ["cooper nettys"], "note": "trained", "trained": true }, { "id": "4", "term": "Zed", "sounds": [], "note": "voice" } ],
            "termCounts": { "x": 3 }, "dismissed": ["y"] } }).to_string()).unwrap();
        let s = Store::open(&d);
        let terms: Vec<&str> = s.config["personal"]["vocabulary"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["term"].as_str().unwrap())
            .collect();
        assert_eq!(terms, ["Kubernetes", "Zed"]);
        assert!(s.config["personal"]["termCounts"].is_null());
        assert_eq!(s.config["vocabAutoCleaned"], json!(true));

        let mut vocab = s.config["personal"]["vocabulary"].as_array().unwrap().clone();
        vocab.push(json!({ "id": "5", "term": "Murmint", "sounds": [], "note": "suggested" }));
        vocab.push(json!({ "id": "6", "term": "LegacyTag", "sounds": [], "note": "auto" }));
        s.set("personal.vocabulary", Value::Array(vocab));
        s.flush();
        let s2 = Store::open(&d);
        let terms2: Vec<&str> = s2.config["personal"]["vocabulary"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["term"].as_str().unwrap())
            .collect();
        assert_eq!(terms2, ["Kubernetes", "Zed", "Murmint", "LegacyTag"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn prune_history_persists_deletions_to_disk() {
        let d = tmp();
        let now = now_ms();
        std::fs::write(
            d.join("history.json"),
            json!([
                { "id": "recent", "ts": now - 2 * 86_400_000, "text": "keep" },
                { "id": "aged", "ts": now - 10 * 86_400_000, "text": "drop" }
            ])
            .to_string(),
        )
        .unwrap();

        let mut s = Store::open(&d);
        assert!(!s.has_unsaved());
        s.set("privacy.retentionDays", json!(5));
        s.prune_history();
        assert_eq!(s.history.len(), 1);
        s.flush();
        let on_disk: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(d.join("history.json")).unwrap()).unwrap();
        assert_eq!(on_disk.len(), 1);
        assert_eq!(on_disk[0]["id"], "recent");

        let mut s2 = Store::open(&d);
        s2.set("privacy.saveHistory", json!(false));
        s2.prune_history();
        assert!(s2.history.is_empty());
        s2.flush();
        let on_disk2: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(d.join("history.json")).unwrap()).unwrap();
        assert!(on_disk2.is_empty());

        let mut s3 = Store::open(&d);
        s3.set("privacy.saveHistory", json!(true));
        s3.flush();
        assert!(Store::open(&d).history.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn degraded_store_preserves_corrupt_or_unreadable_files_across_flush() {
        let d = tmp();
        let bad_hist = "{not valid json";
        std::fs::write(d.join("history.json"), bad_hist).unwrap();
        let mut s = Store::open(&d);
        assert!(s.is_degraded("history.json"));
        assert!(!s.is_degraded("config.json"));
        assert_eq!(std::fs::read_to_string(d.join("history.json.bak")).unwrap(), bad_hist);

        s.add_history(json!({ "id": "new1", "ts": now_ms(), "text": "hello" }));
        s.flush();
        assert_eq!(std::fs::read_to_string(d.join("history.json")).unwrap(), bad_hist);

        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let d2 = tmp();
            let valid_hist = json!([{ "id": "orig", "ts": now_ms(), "text": "keep me" }]).to_string();
            std::fs::write(d2.join("history.json"), &valid_hist).unwrap();
            let lock_file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(d2.join("history.json"))
                .unwrap();
            let mut s2 = Store::open(&d2);
            drop(lock_file);
            assert!(s2.is_degraded("history.json"));
            s2.add_history(json!({ "id": "new2", "ts": now_ms(), "text": "must not overwrite" }));
            s2.flush();
            assert_eq!(std::fs::read_to_string(d2.join("history.json")).unwrap(), valid_hist);
            let _ = std::fs::remove_dir_all(&d2);
        }

        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn set_path_handles_arrays_safely() {
        let mut v = json!({
            "appProfiles": [
                { "id": "slack", "style": "custom-1" },
                { "id": "discord", "style": "casual" }
            ]
        });
        set_path(&mut v, "appProfiles.0.style", json!(""));
        assert!(v["appProfiles"].is_array());
        assert_eq!(v["appProfiles"][0]["style"], "");
        assert_eq!(v["appProfiles"][1]["id"], "discord");

        set_path(&mut v, "appProfiles.99.style", json!("x"));
        set_path(&mut v, "appProfiles.bad.style", json!("x"));
        assert!(v["appProfiles"].is_array());
        assert_eq!(v["appProfiles"].as_array().unwrap().len(), 2);

        set_path(&mut v, "appProfiles.1", json!({ "id": "discord", "style": "natural" }));
        assert_eq!(v["appProfiles"][1]["style"], "natural");
    }

    #[test]
    fn dpapi_failure_returns_error_without_plaintext_fallback() {
        let d = tmp();
        let mut s = Store::open(&d);
        FORCE_DPAPI_FAIL.store(true, std::sync::atomic::Ordering::SeqCst);
        let res = s.try_set_secret("openai", "sk-never-plaintext");
        FORCE_DPAPI_FAIL.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(res.is_err());
        assert!(!s.has_secret("openai"));
        s.flush();
        assert!(!d.join("secrets.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn removed_shipped_models_are_not_readded_on_reopen() {
        let d = tmp();
        let mut s = Store::open(&d);
        let mut providers = s.config["providers"].as_array().unwrap().clone();
        let openai = providers.iter_mut().find(|p| p["id"] == "openai").unwrap();
        let models = openai["models"].as_array().unwrap().clone();
        let removed_id = models[0]["id"].clone();
        let kept: Vec<Value> = models.into_iter().skip(1).collect();
        openai["models"] = Value::Array(kept);
        openai["removedModels"] = json!([removed_id]);
        s.replace_section("providers", Value::Array(providers));
        s.flush();

        let s2 = Store::open(&d);
        let openai2 = s2.provider("openai").unwrap();
        assert!(!openai2["models"].as_array().unwrap().iter().any(|m| m["id"] == removed_id));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_old_custom_instructions_box_becomes_the_first_rule() {
        let d = tmp();
        std::fs::write(
            d.join("config.json"),
            json!({ "formatting": { "customInstruction": "Use British spelling." } }).to_string(),
        )
        .unwrap();
        let s = Store::open(&d);
        let rules = s.config["formatting"]["rules"].as_array().unwrap();
        assert_eq!(
            (rules.len(), rules[0]["text"].as_str(), rules[0]["enabled"].as_bool()),
            (1, Some("Use British spelling."), Some(true))
        );
        assert!(s.config["formatting"]["customInstruction"].is_null());
        let d2 = tmp();
        assert_eq!(
            Store::open(&d2).config["formatting"]["rules"],
            json!([]),
            "a fresh install starts with no rules"
        );
    }

    #[test]
    fn old_style_wording_is_upgraded_and_edited_wording_kept() {
        let d = tmp();
        let old = old_styles();
        let mut styles = defaults()["styles"].as_array().unwrap().clone();
        for s in styles.iter_mut() {
            if s["id"] == "casual" {
                s["instruction"] = old["casual"].as_array().unwrap()[0].clone();
            }
            if s["id"] == "formal" {
                s["instruction"] = json!("My own formal rules.");
            }
        }
        std::fs::write(d.join("config.json"), json!({ "styles": styles }).to_string()).unwrap();
        let s = Store::open(&d);
        let get = |id: &str| {
            s.config["styles"].as_array().unwrap().iter().find(|x| x["id"] == id).unwrap()["instruction"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert!(get("casual").contains("Contractions everywhere"));
        assert_eq!(get("formal"), "My own formal rules.");
    }

    #[test]
    fn providers_ready_only_with_a_key_and_models_are_found_by_ref() {
        let d = tmp();
        let mut s = Store::open(&d);
        assert!(!s.provider_ready("openai"));
        s.set_secret("openai", "k");
        assert!(s.provider_ready("openai"));
        assert!(!s.provider_ready("cloudflare"), "needs an account id too");
        assert!(s.find_model("openai::gpt-4o-mini-transcribe").is_some());
        assert!(s.find_model("openai::nope").is_none());
    }

    #[test]
    fn history_and_usage() {
        let d = tmp();
        let mut s = Store::open(&d);
        s.add_history(json!({ "id": "a", "ts": now_ms() }));
        s.add_history(json!({ "id": "b", "ts": now_ms() - 40 * 86_400_000 }));
        s.prune_history();
        assert_eq!(s.history.len(), 1, "older than 30 days is gone");
        s.update_history("a", &json!({ "edited": "x" }));
        assert_eq!(s.history[0]["edited"], "x");
        s.record_usage("2026-01-01", &json!({ "dictations": 1, "words": 5, "audioSec": 3 }));
        s.record_usage(
            "2026-01-01",
            &json!({ "model": "p::m", "cost": 0.5, "tokensIn": 10, "tokensOut": 2 }),
        );
        assert_eq!(s.usage["days"]["2026-01-01"]["words"], 5.0);
        assert_eq!(s.usage["days"]["2026-01-01"]["byModel"]["p::m"]["requests"], 1.0);
    }
}
