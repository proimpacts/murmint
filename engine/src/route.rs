use crate::store::Store;
use crate::text::local::Signals;
use serde_json::{json, Value};

fn tier_rank(t: &Value) -> i64 {
    match t.as_str() {
        Some("worst") => 0,
        Some("bad" | "cheap") => 1,
        Some("good") => 3,
        Some("best" | "capable") => 4,
        _ => 2, // decent, balanced, or untagged
    }
}

pub struct Cand<'a> {
    pub r: String,
    pub provider: &'a Value,
    pub model: &'a Value,
}

/// A provider running on this PC (Murmint 1, Ollama, LM Studio, a local server), judged by its address.
pub fn on_this_pc(p: &Value) -> bool {
    let base = p["baseUrl"].as_str().unwrap_or("").to_lowercase();
    let host = base.split("://").nth(1).unwrap_or(&base).split(['/', ':']).next().unwrap_or("");
    p["local"] == json!(true) || matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0")
}

pub fn candidates<'a>(store: &'a Store, task: &str) -> Vec<Cand<'a>> {
    let mut out = vec![];
    let local_only = store.config["privacy"]["localOnly"] == json!(true);
    for p in store.providers() {
        let id = p["id"].as_str().unwrap_or("");
        if !store.provider_ready(id) || (local_only && !on_this_pc(p)) {
            continue;
        }
        let removed = p["removedModels"].as_array();
        for m in p["models"].as_array().into_iter().flatten() {
            if removed.is_some_and(|r| r.contains(&m["id"])) {
                continue;
            }
            if m["task"] == task {
                out.push(Cand {
                    r: format!("{id}::{}", m["id"].as_str().unwrap_or("")),
                    provider: p,
                    model: m,
                });
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub ai: bool,
    pub tier: i64,
    pub reason: &'static str,
}
fn local(reason: &'static str) -> Decision {
    Decision { ai: false, tier: 0, reason }
}
fn ai(tier: i64, reason: &'static str) -> Decision {
    Decision { ai: true, tier, reason }
}

pub struct DecideIn<'a> {
    pub force: bool,
    pub signals: &'a Signals,
    pub intensity: i64,
    pub kind: &'a str,
    pub style_id: &'a str,
    pub cfg: &'a Value,
    pub has_model: bool,
}

pub fn decide(d: &DecideIn) -> Decision {
    let fmt = &d.cfg["formatting"];
    let w = d.signals.words;
    let use_ai = fmt["useAI"].as_bool().unwrap_or(true);
    if d.force && d.has_model && use_ai {
        return ai(if w >= 60 { 2 } else { 1 }, "Explicit format request");
    }
    if !use_ai {
        return local("AI formatting is off");
    }
    if d.intensity <= 0 {
        return local("Verbatim mode");
    }
    if !d.has_model {
        return local("No formatting model is configured");
    }
    if w == 0 {
        return local("Empty");
    }
    let email = d.kind == "email";
    let thr =
        d.cfg["models"]["format"]["complexWordThreshold"].as_u64().filter(|n| *n > 0).unwrap_or(90) as usize;
    let heavy = w >= thr
        || (d.signals.list_cue && w > 40)
        || (d.signals.structure_cue && w > 25)
        || (email && w > 60)
        || (d.intensity >= 3 && w > 60);
    let medium = w >= 45 || email || d.intensity >= 3 || d.style_id == "formal" || d.signals.list_cue;
    if heavy {
        ai(2, "Long or structured text")
    } else if medium {
        ai(1, "Needs a careful edit")
    } else {
        ai(0, "Standard formatting")
    }
}

fn price(c: &Cand, field: &str, estimate: f64) -> f64 {
    c.model["price"][field].as_f64().unwrap_or_else(|| {
        if c.provider["keyOptional"] == json!(true) { 0.0 } else { estimate }
    })
}
fn chat_cost(c: &Cand, tin: f64, tout: f64) -> f64 {
    (price(c, "inPerM", 1.0) * tin + price(c, "outPerM", 1.0) * tout) / 1e6
}
fn stt_cost(c: &Cand, sec: f64) -> f64 {
    price(c, "perMinute", 0.01) * sec / 60.0
}

#[derive(Clone)]
struct Item {
    r: String,
    tier: i64,
    cost: f64,
}
// quality, then latency, then price
fn best_first(store: &Store, mut items: Vec<Item>) -> Vec<Item> {
    let lat = |r: &str| store.latency.get(r).copied().unwrap_or(800.0);
    items.sort_by(|a, b| {
        b.tier
            .cmp(&a.tier)
            .then(lat(&a.r).total_cmp(&lat(&b.r)))
            .then(a.cost.total_cmp(&b.cost))
            .then(a.r.cmp(&b.r))
    });
    items
}

/// "Cheapest": the lowest cost first; quality and speed only break ties.
fn cheapest_first(store: &Store, mut items: Vec<Item>) -> Vec<Item> {
    let lat = |r: &str| store.latency.get(r).copied().unwrap_or(800.0);
    items.sort_by(|a, b| {
        a.cost.total_cmp(&b.cost).then(b.tier.cmp(&a.tier)).then(lat(&a.r).total_cmp(&lat(&b.r))).then(a.r.cmp(&b.r))
    });
    items
}

fn dedupe(v: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for r in v {
        if !r.is_empty() && !out.contains(&r) {
            out.push(r);
        }
    }
    out
}
fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

pub fn rank_chat(store: &Store, required_tier: i64, tin: f64, tout: f64) -> Vec<String> {
    let m = &store.config["models"]["format"];
    let cands = candidates(store, "chat");
    if m["routing"] == "manual" {
        let auto_route = m["autoRoute"].as_bool().unwrap_or(false);
        let complex = m["complex"].as_str().unwrap_or("").to_string();
        let heavy = required_tier >= 2 && auto_route && !complex.is_empty();
        let mut refs: Vec<String> = vec![];
        if heavy {
            refs.push(complex.clone());
        }
        refs.push(m["primary"].as_str().unwrap_or("").into());
        refs.extend(strings(&m["fallbacks"]));
        if !heavy && auto_route {
            refs.push(complex);
        }
        return dedupe(refs).into_iter().filter(|r| cands.iter().any(|c| &c.r == r)).collect();
    }
    let items: Vec<Item> = cands
        .iter()
        .map(|c| Item { r: c.r.clone(), tier: tier_rank(&c.model["tier"]), cost: chat_cost(c, tin, tout) })
        .collect();
    let ranked = if m["routing"] == "cheapest" { cheapest_first(store, items) } else { best_first(store, items) };
    ranked.into_iter().map(|i| i.r).collect()
}

pub fn rank_stt(store: &Store, sec: f64) -> Vec<String> {
    let m = &store.config["models"]["stt"];
    let lang = crate::accent::language(&store.config);
    let lang = lang.as_str();
    let live_enabled = store.config["dictation"]["realtime"] != json!(false);
    let cands: Vec<_> = candidates(store, "stt").into_iter()
        .filter(|c| c.model["realtime"] != "live" || (live_enabled && c.provider["type"] != "cloudflare"))
        .collect();
    if m["routing"] == "manual" {
        let mut refs = vec![m["primary"].as_str().unwrap_or("").to_string()];
        refs.extend(strings(&m["fallbacks"]));
        return dedupe(refs).into_iter().filter(|r| cands.iter().any(|c| &c.r == r)).collect();
    }
    let english = lang == "en";
    let items: Vec<Item> = cands
        .iter()
        .filter(|c| c.model["englishOnly"] != json!(true) || english)
        .map(|c| Item { r: c.r.clone(), tier: tier_rank(&c.model["tier"]), cost: stt_cost(c, sec) })
        .collect();
    let ranked = if m["routing"] == "cheapest" { cheapest_first(store, items) } else { best_first(store, items) };
    ranked.into_iter().map(|i| i.r).collect()
}

pub fn streams(store: &Store, r: &str) -> bool {
    store.config["dictation"]["realtime"] != json!(false)
        && store.find_model(r).is_some_and(|(p, m)| {
            p["type"] != "cloudflare" && matches!(m["realtime"].as_str(), Some("live" | "turn"))
        })
}

pub fn batch_refs(store: &Store, refs: Vec<String>) -> Vec<String> {
    refs.into_iter().filter(|r| store.find_model(r).is_some_and(|(_, m)| m["realtime"] != "live")).collect()
}

pub fn est_chat(store: &Store, r: &str, tin: f64, tout: f64) -> f64 {
    candidates(store, "chat").iter().find(|c| c.r == r).map_or(0.0, |c| chat_cost(c, tin, tout))
}
pub fn est_stt(store: &Store, r: &str, sec: f64) -> f64 {
    candidates(store, "stt").iter().find(|c| c.r == r).map_or(0.0, |c| stt_cost(c, sec))
}

pub fn preview(store: &Store) -> Value {
    let (tin, tout) = (300.0, 100.0);
    let mut chat = vec![];
    if let Some(r) = rank_chat(store, 0, tin, tout).first() {
        chat.push(json!({ "label": "Formatting", "ref": r, "cost": est_chat(store, r, tin, tout) }));
    }
    let stt = rank_stt(store, 5.0)
        .first()
        .map(|r| json!({ "ref": r, "cost": est_stt(store, r, 5.0), "stream": streams(store, r) }))
        .unwrap_or(Value::Null);
    json!({ "chat": chat, "stt": stt })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store_with(keys: &[&str]) -> Store {
        let d = std::env::temp_dir().join(format!(
            "murmint-route-{}-{}",
            std::process::id(),
            crate::store::now_ms()
        ));
        let mut s = Store::open(&d);
        for k in keys {
            s.set_secret(k, "k");
        }
        if let Some(ps) = s.config["providers"].as_array_mut() {
            for p in ps {
                if p["id"] == "cloudflare" {
                    p["accountId"] = json!("a".repeat(32));
                }
            }
        }
        s
    }
    fn sig(words: usize) -> Signals {
        Signals { words, ..Default::default() }
    }
    fn dec(cfg: &Value, words: usize, f: impl Fn(&mut DecideIn)) -> Decision {
        let s = sig(words);
        let mut d = DecideIn {
            force: false,
            signals: &s,
            intensity: 2,
            kind: "auto",
            style_id: "natural",
            cfg,
            has_model: true,
        };
        f(&mut d);
        decide(&d)
    }

    #[test]
    fn every_dictation_goes_to_the_model_however_short() {
        let cfg = crate::store::defaults();
        for w in [1, 4, 12, 30] {
            assert!(dec(&cfg, w, |_| {}).ai, "{w} words");
        }
        assert!(!dec(&cfg, 5, |d| d.intensity = 0).ai, "verbatim");
        assert!(!dec(&cfg, 5, |d| d.has_model = false).ai, "nothing set up");
        let mut off = cfg.clone();
        off["formatting"]["useAI"] = json!(false);
        assert!(!dec(&off, 5, |_| {}).ai, "AI formatting off");
        assert_eq!(dec(&cfg, 5, |_| {}).tier, 0);
        assert_eq!(dec(&cfg, 200, |_| {}).tier, 2);
        assert_eq!(dec(&cfg, 20, |d| d.kind = "email").tier, 1);
        assert_eq!(dec(&cfg, 5, |d| d.force = true).reason, "Explicit format request");
    }

    #[test]
    fn the_best_model_comes_first_whatever_it_costs_and_however_short_the_text() {
        let s = store_with(&["openai", "cloudflare"]);
        for words_tier in [0, 1, 2] {
            let r = rank_chat(&s, words_tier, 300.0, 100.0);
            assert!(s.find_model(&r[0]).unwrap().1["tier"] == "best", "tier {words_tier}: {r:?}");
            let tiers: Vec<i64> = r.iter().map(|x| tier_rank(&s.find_model(x).unwrap().1["tier"])).collect();
            assert!(
                tiers.windows(2).all(|w| w[0] >= w[1]),
                "never a weaker model before a stronger one: {tiers:?}"
            );
        }
        assert!(rank_chat(&store_with(&[]), 0, 1.0, 1.0).is_empty());
    }

    #[test]
    fn the_best_speech_model_is_chosen_and_english_only_ones_are_kept_to_english() {
        let s = store_with(&["openai", "cloudflare"]);
        let any = rank_stt(&s, 5.0);
        assert_eq!(s.find_model(&any[0]).unwrap().1["tier"], "best", "{any:?}");
        assert!(
            !any.iter().any(|r| r.contains("whisper-tiny-en")),
            "the English-only model is not used when the language is automatic"
        );
    }

    #[test]
    fn speech_transport_follows_the_selected_model() {
        let mut s = store_with(&["openai", "cloudflare"]);
        s.set("models.stt", json!({
            "routing": "manual", "primary": "cloudflare::@cf/openai/whisper", "fallbacks": []
        }));
        let refs = rank_stt(&s, 5.0);
        assert_eq!(refs, ["cloudflare::@cf/openai/whisper"]);
        assert!(!streams(&s, &refs[0]));
        s.set("models.stt.primary", json!("openai::gpt-live-transcribe"));
        let refs = rank_stt(&s, 5.0);
        assert!(streams(&s, &refs[0]));
        assert!(batch_refs(&s, refs).is_empty());
        s.set("models.stt.routing", json!("cheapest"));
        assert!(!streams(&s, &rank_stt(&s, 5.0)[0]));
    }

    #[test]
    fn a_manual_choice_is_followed() {
        let mut s = store_with(&["openai", "cloudflare"]);
        s.set("models.format", json!({ "primary": "openai::gpt-5.4-mini", "complex": "", "fallbacks": ["cloudflare::@cf/meta/llama-3.1-8b-instruct"], "autoRoute": false, "routing": "manual" }));
        assert_eq!(
            rank_chat(&s, 0, 1.0, 1.0),
            ["openai::gpt-5.4-mini", "cloudflare::@cf/meta/llama-3.1-8b-instruct"]
        );
    }

    #[test]
    fn only_providers_on_this_pc_count_as_local() {
        assert!(on_this_pc(&json!({ "baseUrl": "http://localhost:11434/v1" })));
        assert!(on_this_pc(&json!({ "baseUrl": "http://127.0.0.1:1234/v1" })));
        assert!(on_this_pc(&json!({ "baseUrl": "http://[::1]:11434/v1" })));
        assert!(on_this_pc(&json!({ "baseUrl": "::1" })));
        assert!(on_this_pc(&json!({ "local": true })));
        assert!(on_this_pc(&json!({ "id": "murmint-local" })));
        assert!(!on_this_pc(&json!({ "baseUrl": "https://example.com", "local": true })));
        assert!(!on_this_pc(&json!({ "type": "cloudflare", "baseUrl": "http://127.0.0.1:1234", "local": true })));
        assert!(!on_this_pc(&json!({ "baseUrl": "https://api.openai.com/v1" })));
        assert!(!on_this_pc(&json!({ "baseUrl": "https://localhost.evil.com/v1" })));
    }

    #[test]
    fn removed_models_are_excluded_from_candidates_and_routing() {
        let mut s = store_with(&["openai"]);
        if let Some(ps) = s.config["providers"].as_array_mut() {
            for p in ps {
                if p["id"] == "openai" {
                    p["removedModels"] = json!(["gpt-5.4-mini"]);
                }
            }
        }
        let chat = rank_chat(&s, 0, 300.0, 100.0);
        assert!(!chat.iter().any(|r| r == "openai::gpt-5.4-mini"), "{chat:?}");
        assert!(chat.iter().any(|r| r == "openai::gpt-5-mini"), "{chat:?}");
    }

    #[test]
    fn local_only_leaves_out_cloud_providers() {
        let mut s = store_with(&["openai", "cloudflare"]);
        assert!(!candidates(&s, "chat").is_empty());
        s.set("privacy.localOnly", json!(true));
        assert!(candidates(&s, "chat").is_empty());
    }

    #[test]
    fn cheapest_puts_the_lowest_cost_model_first() {
        let mut s = store_with(&["openai", "cloudflare"]);
        s.set("models.format.routing", json!("cheapest"));
        let ranked = rank_chat(&s, 0, 300.0, 100.0);
        let costs: Vec<f64> = ranked.iter().map(|r| est_chat(&s, r, 300.0, 100.0)).collect();
        assert!(costs.windows(2).all(|w| w[0] <= w[1]), "{ranked:?} {costs:?}");
    }
}
