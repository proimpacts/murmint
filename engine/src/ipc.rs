use crate::core::{self, changed, state_for_renderer};
use crate::hub;
use crate::insertion;
use crate::profiles::Context;
use crate::route;
use crate::session;
use crate::store::now_ms;
use crate::usage::summarize;
use crate::{formatter, native, presets, vocab};
use base64::Engine as _;
use serde_json::{json, Value};

fn b64(v: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(v.as_str().unwrap_or("")).unwrap_or_default()
}

pub fn clean_token(raw: &str) -> String {
    let t = crate::re!(r"(?i)^authorization:\s*").replace(raw.trim(), "").to_string();
    let t = crate::re!(r"(?i)^bearer\s+").replace(&t, "").to_string();
    let t = t.trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string();
    t.split_whitespace().collect()
}
pub fn parse_account_id(raw: &str) -> String {
    let s = raw.trim();
    crate::re!(r"(?i)[0-9a-f]{32}")
        .find(s)
        .ok()
        .flatten()
        .map(|m| m.as_str().to_lowercase())
        .unwrap_or_else(|| s.to_string())
}

fn with_store<T>(f: impl FnOnce(&mut crate::store::Store) -> T) -> T {
    f(&mut core::get().store.lock().unwrap())
}

pub async fn ui(ch: &str, a: &[Value]) -> Value {
    let arg = |i: usize| a.get(i).cloned().unwrap_or(Value::Null);
    match ch {
        "state:get" => state_for_renderer(),
        "config:set" => {
            let path = arg(0).as_str().unwrap_or("").to_string();
            with_store(|s| {
                s.set(&path, arg(1));
                if path.starts_with("privacy.") {
                    s.prune_history();
                }
            });
            changed("config");
            json!(true)
        }
        "provider:save" => {
            let mut p = arg(0);
            if let Some(acc) = p["accountId"].as_str() {
                p["accountId"] = json!(parse_account_id(acc));
            }
            with_store(|s| {
                let mut list = s.providers().clone();
                let id = p["id"].as_str().unwrap_or("").to_string();
                if let Some(i) = list.iter().position(|x| x["id"] == id) {
                    let builtin = list[i]["builtin"].clone();
                    let mut merged = list[i].as_object().cloned().unwrap_or_default();
                    if let Some(po) = p.as_object() {
                        for (k, v) in po {
                            merged.insert(k.clone(), v.clone());
                        }
                    }
                    merged.insert("builtin".into(), builtin);
                    list[i] = Value::Object(merged);
                } else if let Some(ship) = crate::store::defaults()["providers"]
                    .as_array()
                    .and_then(|d| d.iter().find(|x| x["id"] == id).cloned())
                {
                    list.push(ship);
                    let gone: Vec<Value> = s.config["removedProviders"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|x| x != &json!(id))
                        .collect();
                    s.set("removedProviders", Value::Array(gone));
                } else {
                    let mut n = p.as_object().cloned().unwrap_or_default();
                    if id.is_empty() {
                        n.insert(
                            "id".into(),
                            json!(format!("custom-{}", &uuid::Uuid::new_v4().simple().to_string()[..6])),
                        );
                    }
                    let ptype = match n.get("type").and_then(Value::as_str) {
                        Some("anthropic") => "anthropic",
                        _ => "compatible",
                    };
                    n.insert("type".into(), json!(ptype));
                    n.insert("builtin".into(), json!(false));
                    n.insert("enabled".into(), json!(true));
                    if n.get("models").is_none() {
                        n.insert("models".into(), json!([]));
                    }
                    list.push(Value::Object(n));
                }
                s.replace_section("providers", Value::Array(list));
            });
            changed("config");
            json!(true)
        }
        "provider:remove" => {
            let id = arg(0).as_str().unwrap_or("").to_string();
            let removed = with_store(|s| {
                // remember removed built-ins so they don't come back
                let Some(builtin) = s.provider(&id).map(|p| p["builtin"] == json!(true)) else {
                    return false;
                };
                if builtin {
                    let mut gone = s.config["removedProviders"].as_array().cloned().unwrap_or_default();
                    if !gone.contains(&json!(id)) {
                        gone.push(json!(id));
                    }
                    s.set("removedProviders", Value::Array(gone));
                }
                let list: Vec<Value> = s.providers().iter().filter(|x| x["id"] != id).cloned().collect();
                s.replace_section("providers", Value::Array(list));
                s.set_secret(&id, "");
                let alive = |r: &str| r.is_empty() || r.split("::").next() != Some(id.as_str());
                let mut m = s.config["models"].clone();
                for k in ["stt", "format"] {
                    if let Some(f) = m[k]["fallbacks"].as_array() {
                        m[k]["fallbacks"] = Value::Array(
                            f.iter().filter(|r| alive(r.as_str().unwrap_or(""))).cloned().collect(),
                        );
                    }
                }
                for (k, f) in [("stt", "primary"), ("format", "primary"), ("format", "complex")] {
                    if !alive(m[k][f].as_str().unwrap_or("")) {
                        m[k][f] = json!("");
                    }
                }
                s.set("models", m);
                true
            });
            if removed {
                changed("config");
            }
            json!(removed)
        }
        "provider:key" => {
            let id = arg(0).as_str().unwrap_or("").to_string();
            let raw = arg(1).as_str().unwrap_or("").to_string();
            let res = with_store(|s| {
                let cf = s.provider(&id).is_some_and(|p| p["type"] == "cloudflare");
                let key = sanitize_api_key(&raw, cf)?;
                s.try_set_secret(&id, &key)
            });
            match res {
                Ok(()) => {
                    changed("config");
                    json!({ "ok": true })
                }
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        "provider:test" => hub::test(arg(0).as_str().unwrap_or("")).await,
        "provider:presets" => presets::presets(),
        "provider:models" => provider_models(arg(0).as_str().unwrap_or("")).await,
        "provider:list" => {
            let a = arg(0);
            let s_field = |k: &str| a[k].as_str().unwrap_or("").to_string();
            let base_url = s_field("baseUrl");
            let mut key = s_field("key");
            if key.is_empty() {
                if let Some(id) = a["id"].as_str().filter(|id| !id.is_empty()) {
                    let norm = |u: &str| u.trim().trim_end_matches('/').to_string();
                    key = with_store(|st| {
                        st.provider(id)
                            .filter(|p| norm(p["baseUrl"].as_str().unwrap_or("")) == norm(&base_url))
                            .map(|_| st.get_secret(id))
                            .unwrap_or_default()
                    });
                }
            }
            list_models(&base_url, &key, s_field("type") == "anthropic", a["free"] == json!(true)).await
        }
        "keys:capture" => {
            if arg(0) == json!(true) {
                native::set_combos(&json!({}));
            } else {
                core::apply_hotkeys();
            }
            json!(true)
        }
        "keys:down" => {
            let r = native::handle("native_down", &json!({ "id": 0 })).await.unwrap_or(json!({}));
            json!(r["keys"]
                .as_array()
                .map(|a| a
                    .iter()
                    .filter_map(|c| native::name_of_vk(c.as_i64().unwrap_or(0) as i32))
                    .collect::<Vec<_>>())
                .unwrap_or_default())
        }
        "history:clear" => {
            with_store(|s| s.clear_history());
            changed("history");
            json!(true)
        }
        "history:delete" => {
            with_store(|s| s.delete_history(arg(0).as_str().unwrap_or("")));
            changed("history");
            json!(true)
        }
        "history:query" => with_store(|s| query_history(&s.history, &arg(0))),
        "clipboard:write" => {
            let t = arg(0).as_str().unwrap_or("").to_string();
            let ok = tokio::task::spawn_blocking(move || insertion::write_clipboard_text(&t))
                .await
                .unwrap_or(false);
            json!(ok)
        }
        "overlay:raise" => {
            crate::native::raise_under_taskbar(arg(0).as_i64().unwrap_or(0) as isize);
            json!({})
        }
        "app:trim" => {
            let pids: Vec<u32> = arg(0)
                .as_array()
                .map(|a| a.iter().filter_map(|p| p.as_u64().map(|n| n as u32)).collect())
                .unwrap_or_default();
            // the engine listens for the shortcut, so it stays in memory and answers at once
            let mut n = 0;
            for p in pids {
                n += usize::from(crate::native::trim_working_set(p));
            }
            json!({ "trimmed": n })
        }
        "app:memory" => {
            let pids: Vec<u32> = arg(0)
                .as_array()
                .map(|a| a.iter().filter_map(|p| p.as_u64().map(|n| n as u32)).collect())
                .unwrap_or_default();
            json!({ "engine": crate::native::private_memory_mb(0), "pids": pids.iter().map(|p| crate::native::private_memory_mb(*p)).collect::<Vec<f64>>() })
        }
        "usage:summary" => with_store(|s| summarize(&s.usage, now_ms(), 14)),
        "usage:reset" => {
            with_store(|s| {
                s.usage = json!({ "days": {} });
                s.touch("usage.json");
            });
            changed("usage");
            json!(true)
        }
        "router:preview" => with_store(|s| route::preview(s)),
        "vocab:train" => train_take(&arg(0)).await,
        "format:try" => {
            let o = arg(0);
            let ctx = {
                let c = core::get().store.lock().unwrap().config.clone();
                let style = c["styles"]
                    .as_array()
                    .and_then(|st| st.iter().find(|s| s["id"] == o["styleId"]).or(st.first()))
                    .cloned()
                    .unwrap_or(json!({}));
                let ctx = Context {
                    app_name: String::new(),
                    kind: o["kind"].as_str().unwrap_or("auto").to_string(),
                    style,
                    intensity: o["intensity"]
                        .as_i64()
                        .unwrap_or_else(|| c["formatting"]["intensity"].as_i64().unwrap_or(2)),
                    instruction: o["instruction"].as_str().unwrap_or("").to_string(),
                    forced: false,
                    profile_id: None,
                    terminal: false,
                    before: String::new(),
                    title: String::new(),
                };
                ctx
            };
            // with a program given, the context is worked out the way a dictation in that app would be
            let ctx = match o["process"].as_str() {
                Some(process) => crate::profiles::resolve_context(
                    &core::get().store.lock().unwrap().config,
                    &json!({ "process": process, "title": o["title"].as_str().unwrap_or("") }),
                ),
                None => ctx,
            };
            let mut ctx = ctx;
            ctx.before = o["before"].as_str().unwrap_or("").to_string();
            let out = formatter::format(o["text"].as_str().unwrap_or(""), &ctx, false).await;
            json!({ "text": out.text, "route": out.route, "reason": out.reason, "cost": out.cost, "ref": out.r })
        }
        "session:action" => {
            session::action(arg(0).as_str().unwrap_or("")).await;
            Value::Null
        }
        "session:undo" => insertion::undo().await,
        "clipboard:enable" => {
            crate::clipboard::ON.store(arg(0) == json!(true), std::sync::atomic::Ordering::Relaxed);
            Value::Null
        }
        // a pick from Better Clipboard: back to the window you were in, then type it there
        "clipboard:paste" => {
            let (hwnd, text) = (arg(0)["hwnd"].as_i64().unwrap_or(0), arg(0)["text"].as_str().unwrap_or("").to_string());
            let _ = tokio::task::spawn_blocking(move || crate::clipboard::refocus(hwnd)).await;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            json!(insertion::insert(&text).await.ok)
        }
        // a GIF (or any file) from Better Clipboard: on the clipboard as a file, back to the window, then Ctrl+V
        "clipboard:paste_file" => {
            let hwnd = arg(0)["hwnd"].as_i64().unwrap_or(0);
            let path = arg(0)["path"].as_str().unwrap_or("").to_string();
            let ok = tokio::task::spawn_blocking(move || crate::clipboard::write_files(&[path])).await.unwrap_or(false);
            let _ = tokio::task::spawn_blocking(move || crate::clipboard::refocus(hwnd)).await;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            if ok {
                native::handle("native_paste", &json!({})).await;
            }
            json!(ok)
        }
        // a picture the shell already put on the clipboard: back to the window, then Ctrl+V
        "clipboard:paste_current" => {
            let hwnd = arg(0)["hwnd"].as_i64().unwrap_or(0);
            let _ = tokio::task::spawn_blocking(move || crate::clipboard::refocus(hwnd)).await;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            native::handle("native_paste", &json!({})).await;
            json!(true)
        }
        "local:status" => crate::murmint1::status(),
        "local:download" => crate::murmint1::download(),
        "local:cancel" => {
            crate::murmint1::cancel();
            Value::Null
        }
        "local:use" => json!({ "ok": crate::murmint1::use_it().await }),
        "local:start" => json!({ "ok": crate::murmint1::start().await }),
        "account:get" => crate::account::user(),
        "account:import" => crate::account::import(&arg(0)),
        "account:signIn" => crate::account::sign_in().await,
        "account:cancel" => {
            crate::account::cancel();
            Value::Null
        }
        "account:signOut" => crate::account::sign_out().await,
        "account:verify" => crate::account::verify().await,
        "update:check" => crate::updater::check_now().await,
        "update:get" => crate::updater::available().unwrap_or(Value::Null),
        "update:download" => crate::updater::download().await,
        "site:icon" => crate::site_icons::icon(arg(0).as_str().unwrap_or("")).await,
        "session:lock" => {
            session::LOCKED.store(arg(0) == json!(true), std::sync::atomic::Ordering::Relaxed);
            Value::Null
        }
        "rec:preview" => json!(
            session::preview(b64(&arg(0)["wav_b64"]), arg(0)["durationSec"].as_f64().unwrap_or(0.0)).await
        ),
        "secrets:pending" => with_store(|s| {
            json!(s
                .pending_secrets()
                .into_iter()
                .map(|(id, v)| json!({ "id": id, "v": v }))
                .collect::<Vec<_>>())
        }),
        "secrets:import" => {
            let id = arg(0).as_str().unwrap_or("").to_string();
            let val = arg(1).as_str().unwrap_or("").to_string();
            match with_store(|s| s.try_set_secret(&id, &val)) {
                Ok(()) => {
                    changed("config");
                    json!({ "ok": true })
                }
                Err(e) => json!({ "ok": false, "error": e }),
            }
        }
        "dev:seed" if std::env::var("MURMINT_DEV").is_ok() => {
            let o = arg(0);
            with_store(|s| {
                if let Some(sec) = o["secret"].as_object() {
                    let id = sec["id"].as_str().unwrap_or("");
                    s.set_secret(id, sec["key"].as_str().unwrap_or(""));
                    if let Some(ps) = s.config["providers"].as_array_mut() {
                        for p in ps {
                            if p["id"] == id {
                                p["accountId"] = sec["accountId"].clone();
                            }
                        }
                    }
                }
                for h in o["history"].as_array().cloned().unwrap_or_default() {
                    s.history.push(h);
                }
                s.touch("history.json");
                if let Some(d) = o["days"].as_object() {
                    for (k, v) in d {
                        s.usage["days"][k] = v.clone();
                    }
                    s.touch("usage.json");
                }
                if !o["personal"].is_null() {
                    s.set("personal", o["personal"].clone());
                }
            });
            changed("config");
            json!(true)
        }
        _ => json!({ "error": format!("unknown channel {ch}") }),
    }
}

pub fn send(ch: &str, a: &[Value]) {
    let arg = |i: usize| a.get(i).cloned().unwrap_or(Value::Null);
    match ch {
        "rec:chunk" => session::on_chunk(b64(&arg(0))),
        "rec:boundary" => session::on_boundary(arg(0)["final"] == json!(true), arg(0)["gapMs"].as_u64()),
        "rec:autostop" => session::on_auto_stop(),
        "rec:segment" => {
            let s = arg(0);
            session::on_segment(session::Seg {
                wav: b64(&s["wav_b64"]),
                duration_sec: s["durationSec"].as_f64().unwrap_or(0.0),
                peak: s["peak"].as_f64().unwrap_or(0.0),
                last: s["final"] == json!(true),
                gap_ms: s["gapMs"].as_u64(),
            });
        }
        "rec:error" => session::rec_error(arg(0).as_str().unwrap_or("Something went wrong")),
        _ => {}
    }
}

async fn provider_models(id: &str) -> Value {
    let (p, key) = {
        let s = core::get().store.lock().unwrap();
        (s.provider(id).cloned(), s.get_secret(id))
    };
    let Some(p) = p else { return json!({ "ok": false, "error": "Add the base URL first" }) };
    list_models(p["baseUrl"].as_str().unwrap_or(""), &key, p["type"] == "anthropic", p["free"] == json!(true)).await
}

fn sanitize_api_key(raw: &str, cloudflare: bool) -> Result<String, String> {
    let stripped: String = raw
        .chars()
        .filter(|c| !matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}' | '\u{2060}'))
        .collect();
    let key = if cloudflare {
        clean_token(&stripped)
    } else {
        crate::re!(r"(?i)^bearer\s+").replace(stripped.trim(), "").trim().to_string()
    };
    if key.chars().any(|c| !c.is_ascii() || c.is_ascii_control()) {
        return Err("API key contains invalid characters".to_string());
    }
    Ok(key)
}

fn query_history(history: &[Value], opts: &Value) -> Value {
    let q = opts["query"].as_str().unwrap_or("").trim().to_lowercase();
    let offset = opts["offset"].as_u64().unwrap_or(0) as usize;
    let limit = opts["limit"].as_u64().unwrap_or(100).clamp(1, 500) as usize;

    let all_total = history.len();
    let all_words: u64 = history.iter().map(|h| h["words"].as_u64().unwrap_or(0)).sum();

    let matched: Vec<&Value> = history
        .iter()
        .filter(|h| {
            if q.is_empty() {
                return true;
            }
            let text = h["text"].as_str().unwrap_or("");
            let edited = h["edited"].as_str().unwrap_or("");
            let app = h["app"].as_str().unwrap_or("");
            format!("{text} {edited} {app}").to_lowercase().contains(&q)
        })
        .collect();

    let total = matched.len();
    let total_words: u64 = matched.iter().map(|h| h["words"].as_u64().unwrap_or(0)).sum();
    let items: Vec<Value> = matched.into_iter().skip(offset).take(limit).cloned().collect();
    let has_more = offset.saturating_add(items.len()) < total;

    json!({
        "items": items,
        "total": total,
        "totalWords": total_words,
        "allTotal": all_total,
        "allWords": all_words,
        "hasMore": has_more,
    })
}

/// The models a server offers, for a saved service or one still being filled in.
async fn list_models(base: &str, key: &str, anthropic: bool, free: bool) -> Value {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return json!({ "ok": false, "error": "Add the base URL first" });
    }
    let local = base.contains("localhost") || base.contains("127.0.0.1") || base.contains("[::1]");
    let mut all_models: Vec<Value> = Vec::new();
    let mut seen_ids = std::collections::HashSet::<String>::new();
    let mut seen_cursors = std::collections::HashSet::<String>::new();
    let mut after_id: Option<String> = None;
    let max_pages = if anthropic { 10 } else { 1 };

    for _ in 0..max_pages {
        let mut req = crate::CLIENT
            .get(format!("{base}/models"))
            .timeout(std::time::Duration::from_secs(15));
        if anthropic {
            let mut q: Vec<(&str, &str)> = vec![("limit", "1000")];
            if let Some(cursor) = after_id.as_deref() {
                q.push(("after_id", cursor));
            }
            req = req
                .query(&q)
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        } else if !key.is_empty() {
            req = req.bearer_auth(key);
        }
        let res = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                let error = if e.is_connect() {
                    "Could not reach the server. Is it running?".to_string()
                } else {
                    e.without_url().to_string()
                };
                return json!({ "ok": false, "error": error });
            }
        };
        let st = res.status().as_u16();
        if !(200..300).contains(&st) {
            let error = if st == 401 || st == 403 {
                "The provider rejected the key. Save a valid key first.".to_string()
            } else {
                format!("The provider answered {st}")
            };
            return json!({ "ok": false, "error": error });
        }
        let Ok(body) = res.json::<Value>().await else {
            return json!({ "ok": false, "error": "The server returned an invalid model list." });
        };
        let valid_shape = body.is_array() || body["data"].is_array() || body["models"].is_array();
        if !valid_shape {
            return json!({ "ok": false, "error": "The server returned an unexpected model list format." });
        }
        for m in presets::parse_model_list(&body, local || free) {
            if let Some(id) = m["id"].as_str() {
                if seen_ids.insert(id.to_string()) {
                    all_models.push(m);
                }
            }
        }
        if !anthropic || body["has_more"] != json!(true) {
            break;
        }
        let next = body["last_id"].as_str().unwrap_or("").trim().to_string();
        if next.is_empty() || !seen_cursors.insert(next.clone()) {
            break;
        }
        after_id = Some(next);
    }

    all_models.sort_by(|a, b| {
        a["id"].as_str().unwrap_or("").to_lowercase().cmp(&b["id"].as_str().unwrap_or("").to_lowercase())
    });
    json!({ "ok": true, "models": all_models })
}

async fn train_take(t: &Value) -> Value {
    let (wav, dur, term) = (
        b64(&t["wav_b64"]),
        t["durationSec"].as_f64().unwrap_or(0.0),
        t["term"].as_str().unwrap_or("").to_string(),
    );
    let (refs, lang) = {
        let s = core::get().store.lock().unwrap();
        (route::rank_stt(&s, dur), crate::accent::language(&s.config))
    };
    match hub::transcribe(&refs, &wav, dur, if lang == "auto" { None } else { Some(lang.as_str()) }, "").await
    {
        Ok(r) => match vocab::clean_heard(&r.text, &term) {
            None => {
                json!({ "ok": true, "heard": r.text.trim(), "variant": null, "exact": false, "empty": true })
            }
            Some((exact, variant)) => {
                json!({ "ok": true, "heard": r.text.trim(), "variant": variant, "exact": exact })
            }
        },
        Err(e) => {
            let error = if e.not_configured {
                "Add an API key under Models first, so Murmint can listen to your recording.".to_string()
            } else {
                e.message
            };
            json!({ "ok": false, "error": error })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pasted_tokens_and_account_ids_are_cleaned() {
        assert_eq!(clean_token("  Authorization: Bearer \"abc def\"  "), "abcdef");
        assert_eq!(clean_token("bearer xyz"), "xyz");
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(parse_account_id(&format!("https://dash.cloudflare.com/{id}/ai")), id);
        assert_eq!(parse_account_id("nope"), "nope");
    }

    #[test]
    fn api_key_sanitization_strips_zero_width_and_rejects_invalid_chars() {
        assert_eq!(sanitize_api_key("  Bearer \u{200B}sk-abc123  ", false).unwrap(), "sk-abc123");
        assert!(sanitize_api_key("sk-abc\ndef", false).is_err());
        assert!(sanitize_api_key("sk-abc—def", false).is_err());
        assert_eq!(sanitize_api_key("", false).unwrap(), "");
    }

    #[test]
    fn query_history_searches_and_pages_beyond_300_entries() {
        let rows: Vec<Value> = (0..305)
            .map(|i| {
                json!({
                    "id": format!("id-{i}"),
                    "text": if i == 304 { "needle old entry" } else { "ordinary dictation" },
                    "app": "Notepad",
                    "words": 2
                })
            })
            .collect();

        let p1 = query_history(&rows, &json!({ "query": "", "offset": 0, "limit": 200 }));
        assert_eq!(p1["allTotal"], 305);
        assert_eq!(p1["allWords"], 610);
        assert_eq!(p1["total"], 305);
        assert_eq!(p1["items"].as_array().unwrap().len(), 200);
        assert_eq!(p1["hasMore"], true);

        let p2 = query_history(&rows, &json!({ "query": "", "offset": 200, "limit": 200 }));
        assert_eq!(p2["items"].as_array().unwrap().len(), 105);
        assert_eq!(p2["hasMore"], false);

        let search = query_history(&rows, &json!({ "query": "needle", "offset": 0, "limit": 50 }));
        assert_eq!(search["total"], 1);
        assert_eq!(search["totalWords"], 2);
        assert_eq!(search["allTotal"], 305);
        assert_eq!(search["items"][0]["id"], "id-304");
        assert_eq!(search["hasMore"], false);
    }

    #[tokio::test]
    async fn list_models_paginates_anthropic_and_rejects_malformed_200() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let body = if first_line.contains("/bad/models") {
                    "<html>login</html>".to_string()
                } else if first_line.contains("after_id=m2") {
                    json!({
                        "data": [
                            { "id": "m2", "display_name": "Model 2" },
                            { "id": "m3", "display_name": "Model 3" }
                        ],
                        "has_more": false
                    })
                    .to_string()
                } else {
                    json!({
                        "data": [
                            { "id": "m1", "display_name": "Model 1" },
                            { "id": "m2", "display_name": "Model 2" }
                        ],
                        "has_more": true,
                        "last_id": "m2"
                    })
                    .to_string()
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });

        let ok = list_models(&format!("http://{addr}"), "k", true, false).await;
        assert_eq!(ok["ok"], true);
        let ids: Vec<&str> = ok["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["m1", "m2", "m3"]);

        let bad = list_models(&format!("http://{addr}/bad"), "k", false, false).await;
        assert_eq!(bad["ok"], false);
        srv.abort();
    }
}
