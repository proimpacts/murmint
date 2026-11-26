use crate::text::Regex;
use serde_json::{json, Value};

pub fn norm_proc(p: &str) -> String {
    let t = p.to_lowercase();
    let t = t.rsplit(['\\', '/']).next().unwrap_or("").to_string();
    t.strip_suffix(".exe").map(|s| s.to_string()).unwrap_or(t)
}

// Known by process alone: a window title can match another app's profile, and that profile may be switched off.
const TERMINALS: &[&str] = &[
    "windowsterminal", "cmd", "powershell", "pwsh", "wezterm-gui", "alacritty", "conhost", "openconsole", "mintty",
    "putty", "kitty", "tabby", "hyper", "wsl", "ubuntu", "bash", "nu", "rio", "warp",
];

pub fn match_profile<'a>(cfg: &'a Value, app: &Value) -> Option<&'a Value> {
    if app.is_null() {
        return None;
    }
    let proc = norm_proc(app["process"].as_str().unwrap_or(""));
    let title = app["title"].as_str().unwrap_or("").to_lowercase();
    let (mut best, mut best_score) = (None, 0);
    for p in cfg["appProfiles"].as_array()? {
        let id = p["id"].as_str().unwrap_or("");
        if cfg["appOverrides"][id]["enabled"] == json!(false) {
            continue;
        }
        let proc_hit = !proc.is_empty()
            && p["process"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| norm_proc(x.as_str().unwrap_or("")) == proc));
        let title_hit = !title.is_empty()
            && p["title"].as_array().is_some_and(|a| {
                a.iter().any(|rx| {
                    Regex::new(&format!("(?i){}", rx.as_str().unwrap_or("")))
                        .map(|r| r.is_match(&title).unwrap_or(false))
                        .unwrap_or(false)
                })
            });
        let score = if title_hit {
            3
        } else if proc_hit {
            if id == "browser" {
                1
            } else {
                2
            }
        } else {
            0
        };
        // user apps beat built-ins
        let score = if score > 0 && p["custom"] == json!(true) { score + 1 } else { score };
        if score > best_score {
            best = Some(p);
            best_score = score;
        }
    }
    best
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Context {
    pub app_name: String,
    pub kind: String,
    pub style: Value,
    pub intensity: i64,
    pub instruction: String,
    pub forced: bool,
    pub profile_id: Option<String>,
    /// Message context (when enabled).
    pub before: String,
    /// A terminal, where a pasted line break runs the command.
    pub terminal: bool,
    /// The window being typed into, for matching it to a coding-agent chat.
    pub title: String,
}

// Built-ins only apply their style once added on the Apps page; the kind (chat/email/code) always applies.
fn is_added(profile: &Value, ov: &Value) -> bool {
    profile["custom"] == json!(true)
        || ov["added"] == json!(true)
        || ov["style"].as_str().is_some_and(|s| !s.is_empty())
        || ov["intensity"].is_number()
        || ov["instruction"].as_str().is_some_and(|s| !s.is_empty())
}

pub fn resolve_context(cfg: &Value, app: &Value) -> Context {
    let profile = match_profile(cfg, app);
    let id = profile.and_then(|p| p["id"].as_str()).unwrap_or("");
    let ov = &cfg["appOverrides"][id];
    let pick = |k: &str| -> Value {
        let o = &ov[k];
        if !o.is_null() && o != "" {
            return o.clone();
        }
        profile.map(|p| p[k].clone()).unwrap_or(Value::Null)
    };
    let added = profile.is_some_and(|p| is_added(p, ov));
    let own = |k: &str| if added { pick(k) } else { Value::Null };
    let style_id = own("style")
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg["formatting"]["style"].as_str().unwrap_or("natural").to_string());
    let styles = cfg["styles"].as_array().cloned().unwrap_or_default();
    let style = styles
        .iter()
        .find(|s| s["id"] == style_id)
        .or(styles.first())
        .cloned()
        .unwrap_or(json!({ "id": "natural", "name": "Natural", "instruction": "" }));
    let intensity =
        own("intensity").as_i64().unwrap_or_else(|| cfg["formatting"]["intensity"].as_i64().unwrap_or(2));
    Context {
        app_name: profile
            .and_then(|p| p["name"].as_str())
            .map(|s| s.to_string())
            .or_else(|| app["process"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "Unknown app".into()),
        kind: pick("kind").as_str().unwrap_or("auto").to_string(),
        style,
        intensity,
        instruction: pick("instruction").as_str().unwrap_or("").to_string(),
        forced: false,
        profile_id: profile.and_then(|p| p["id"].as_str()).map(|s| s.to_string()),
        before: String::new(),
        terminal: TERMINALS.contains(&norm_proc(app["process"].as_str().unwrap_or("")).as_str()),
        title: app["title"].as_str().unwrap_or("").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cfg() -> Value {
        crate::store::defaults()
    }
    #[test]
    fn apps_are_recognised_by_process_and_by_window_title() {
        let c = cfg();
        let id = |app: Value| match_profile(&c, &app).and_then(|p| p["id"].as_str().map(|s| s.to_string()));
        assert_eq!(id(json!({ "process": "Discord", "title": "x" })), Some("discord".into()));
        assert_eq!(
            id(json!({ "process": "chrome", "title": "Inbox - Gmail" })),
            Some("gmail".into()),
            "a webmail tab beats the plain browser"
        );
        assert_eq!(id(json!({ "process": "msedge", "title": "News" })), Some("browser".into()));
        assert_eq!(id(json!({ "process": "OUTLOOK.EXE", "title": "" })), Some("outlook".into()));
        assert_eq!(id(json!({ "process": "unknownthing", "title": "" })), None);
        assert_eq!(norm_proc("C:\\Apps\\Discord.exe"), "discord");
    }
    #[test]
    fn context_uses_profile_then_overrides_then_globals() {
        let mut c = cfg();
        let discord = json!({ "process": "Discord" });
        let ctx = resolve_context(&c, &discord);
        assert_eq!((ctx.kind.as_str(), ctx.style["id"].as_str().unwrap()), ("chat", "natural"));
        c["appOverrides"] = json!({ "discord": { "added": true } });
        assert_eq!(resolve_context(&c, &discord).style["id"], "casual");
        c["appOverrides"] = json!({ "discord": { "style": "formal" } });
        assert_eq!(resolve_context(&c, &discord).style["id"], "formal");
        let plain = resolve_context(&c, &json!({ "process": "zzz" }));
        assert_eq!(
            (plain.kind.as_str(), plain.style["id"].as_str().unwrap(), plain.intensity),
            ("auto", "natural", 2)
        );
    }
    #[test]
    fn your_own_app_gets_its_style_and_an_app_can_be_switched_off() {
        let mut c = cfg();
        c["appProfiles"].as_array_mut().unwrap().push(json!({
            "id": "app-zed", "name": "Zed", "process": ["zed"], "title": [], "kind": "auto",
            "style": "technical", "intensity": null, "instruction": "", "custom": true
        }));
        let zed = json!({ "process": "Zed.exe", "title": "" });
        let ctx = resolve_context(&c, &zed);
        assert_eq!((ctx.app_name.as_str(), ctx.style["id"].as_str().unwrap()), ("Zed", "technical"));
        c["appOverrides"] = json!({ "app-zed": { "style": "formal" }, "slack": { "enabled": false } });
        assert_eq!(resolve_context(&c, &zed).style["id"], "formal");
        let slack = resolve_context(&c, &json!({ "process": "Slack" }));
        assert_eq!((slack.profile_id, slack.style["id"].as_str().unwrap()), (None, "natural"));
    }
    #[test]
    fn an_app_you_added_beats_the_built_in_profile_for_the_same_program() {
        let mut c = cfg();
        let obsidian = json!({ "process": "Obsidian.exe", "title": "" });
        assert_eq!(
            match_profile(&c, &obsidian).unwrap()["id"],
            "notion",
            "the built-in profile covers Obsidian"
        );
        c["appProfiles"].as_array_mut().unwrap().push(json!({
            "id": "app-obsidian", "name": "Obsidian", "process": ["obsidian"], "title": [], "kind": "document",
            "style": "casual", "intensity": null, "instruction": "", "custom": true
        }));
        assert_eq!(match_profile(&c, &obsidian).unwrap()["id"], "app-obsidian");
        let chrome = json!({ "process": "chrome", "title": "News" });
        c["appProfiles"].as_array_mut().unwrap().push(json!({
            "id": "app-chrome", "name": "Google Chrome", "process": ["chrome"], "title": [], "kind": "auto",
            "style": "", "intensity": null, "instruction": "", "custom": true
        }));
        assert_eq!(match_profile(&c, &chrome).unwrap()["id"], "app-chrome");
    }

}
