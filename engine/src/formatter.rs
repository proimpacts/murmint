use crate::core;
use crate::hub;
use crate::learn::hint_terms;
use crate::profiles::Context;
use crate::prompt::{build_system_prompt, sanitize, shots, wrap_user_ctx, PromptIn};
use crate::route::{self, decide, DecideIn, Decision};
use crate::text::{
    self,
    local::{local_format, LocalOut},
    smart,
};
use crate::usage::estimate_tokens;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

pub struct Prep {
    pub raw: String,
    pub ctx: Context,
    pub formalize: bool,
    pub cleaned: LocalOut,
    pub verdict: Decision,
}

#[derive(Default, Clone, Debug)]
#[allow(dead_code)]
pub struct FormatOut {
    pub local_text: String,
    pub text: String,
    pub route: String,
    pub cost: f64,
    pub r: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub reason: String,
    pub ai_error: Option<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

pub fn prep(cfg: &Value, has_model: bool, personal: &Value, raw: &str, ctx: &Context) -> Prep {
    let fmt = &cfg["formatting"];
    let ctx = ctx.clone();
    let raw = raw.to_string();
    let style_id = ctx.style["id"].as_str().unwrap_or("").to_string();
    let formalize = style_id == "formal" || style_id == "professional";
    let single = smart::split_sentences(&raw).len() <= 1;
    let b = |k: &str, d: bool| fmt[k].as_bool().unwrap_or(d);
    let opts = text::local::LocalOpts {
        kind: ctx.kind.clone(),
        intensity: ctx.intensity,
        language: cfg["dictation"]["language"].as_str().unwrap_or("auto").to_string(),
        remove_fillers: b("removeFillers", true),
        spoken_commands: b("spokenCommands", true),
        auto_capitalize: b("autoCapitalize", true),
        terminal_punct: if ctx.kind == "chat" && single { Some(false) } else { None },
        email_format: b("emailFormat", false),
        auto_lists: b("autoLists", false),
        grammar: b("grammar", false),
        formalize,
        expand_contractions: style_id == "formal",
        humanize: fmt["humanize"].as_str().unwrap_or("off").to_string(),
        excluded: strings(&fmt["excludedChars"]),
        self_correct: true,
    };
    let cleaned = local_format(&raw, &opts, personal);
    let verdict = if !cleaned.text.is_empty() {
        decide(&DecideIn {
            force: ctx.forced,
            signals: &cleaned.signals,
            intensity: ctx.intensity,
            kind: &ctx.kind,
            style_id: &style_id,
            cfg,
            has_model,
        })
    } else {
        Decision { ai: false, tier: 0, reason: "Empty" }
    };
    Prep { raw, ctx, formalize, cleaned, verdict }
}

fn snapshot() -> (Value, Value, bool) {
    let s = core::get().store.lock().unwrap();
    (s.config.clone(), s.config["personal"].clone(), !route::candidates(&s, "chat").is_empty())
}

static CACHE: LazyLock<Mutex<HashMap<u64, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
fn key_of(parts: &[&str]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in parts {
        p.hash(&mut h);
        0u8.hash(&mut h);
    }
    h.finish()
}

fn drop_unsaid_terms(out: &str, input: &str, personal: &serde_json::Value) -> String {
    let (lo_in, mut t) = (input.to_lowercase(), out.to_string());
    for e in crate::vocab::entries(personal) {
        let term = e.term.trim();
        if term.chars().count() < 3 || lo_in.contains(&term.to_lowercase()) {
            continue;
        }
        let Ok(re) = fancy_regex::Regex::new(&format!(
            r"(?i)(?<![\p{{L}}\p{{N}}]){}(?![\p{{L}}\p{{N}}])",
            fancy_regex::escape(term)
        )) else {
            continue;
        };
        if re.is_match(&t).unwrap_or(false) {
            t = re.replace_all(&t, "").to_string();
            t = text::replace_lit(crate::re!(r"[ \t]{2,}"), &t, " ");
            t = text::replace_all(crate::re!(r"[ \t]+([,.;!?])"), &t, |c| text::grp(c, 1).to_string());
            t = text::replace_all(crate::re!(r"[,;]\s*([.!?])"), &t, |c| text::grp(c, 1).to_string());
        }
    }
    t.trim().to_string()
}

// The text already in the box is context only, but a model sometimes copies sentences from it into the reply (often
// under a "Context:" heading). Lines that come from the box and not from what was said are dropped, and so is a
// heading left with nothing under it.
fn drop_context_lines(out: &str, before: &str, said: &str) -> String {
    let norm = |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let (box_text, said) = (norm(before), norm(said));
    let kept: Vec<&str> = out
        .lines()
        .filter(|line| {
            let item = norm(line.trim_start().trim_start_matches(|c: char| c == '-' || c == '*' || c == '•' || c.is_ascii_digit() || c == '.' || c == ')'));
            !(item.split(' ').count() >= 4 && box_text.contains(&item) && !said.contains(&item))
        })
        .collect();
    let mut lines: Vec<&str> = vec![];
    for (i, line) in kept.iter().enumerate() {
        let heading = line.trim_end().ends_with(':') && line.trim().split_whitespace().count() <= 3;
        let next = kept[i + 1..].iter().find(|l| !l.trim().is_empty());
        let empty = next.map_or(true, |n| n.trim_end().ends_with(':') && n.trim().split_whitespace().count() <= 3);
        if heading && empty {
            continue;
        }
        lines.push(line);
    }
    let mut t = lines.join("
");
    while t.contains("


") {
        t = t.replace("


", "

");
    }
    t.trim().to_string()
}

fn rules_text(fmt: &serde_json::Value) -> String {
    fmt["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["enabled"] != json!(false))
        .filter_map(|r| r["text"].as_str().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()))
        .enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t))
        .collect::<Vec<_>>()
        .join("\n")
}

struct Request {
    system: String,
    examples: Vec<(String, String)>,
    better: bool,
    rules: String,
}

// The system prompt and examples for one formatting request. Only the vocabulary, a spoken list and message context
// depend on what was said, so `warm` can build the same prompt before the person has finished talking.
fn compose(cfg: &Value, ctx: &Context, vocabulary: &[String], wants_list: bool, has_context: bool) -> Request {
    let fmt = &cfg["formatting"];
    let privacy = &cfg["privacy"];
    let excluded = strings(&fmt["excludedChars"]);
    // rewritten prompts drift further from the input; loosen the checks
    let better = cfg["formatting"]["betterPrompts"] != json!(false)
        && crate::prompt::is_ai_prompt(&ctx.kind, &ctx.app_name);
    let rules = rules_text(fmt);
    let mut system = build_system_prompt(&PromptIn {
        style: &ctx.style,
        intensity: ctx.intensity,
        kind: &ctx.kind,
        app_name: if privacy["sendAppContext"].as_bool().unwrap_or(true) { &ctx.app_name } else { "" },
        instruction: &ctx.instruction,
        global_instruction: &rules,
        vocabulary,
        language: cfg["dictation"]["language"].as_str().unwrap_or("auto"),
        excluded: &excluded,
        humanize: fmt["humanize"].as_str().unwrap_or("off"),
        wants_list,
        has_context,
        better_prompts: better,
        auto_detect: fmt["autoLists"] == json!(true) && fmt["spokenCommands"] != json!(false),
    });
    // Advanced prompting: the end of the person's coding-agent chat, so the prompt can name what they mean
    if better && fmt["advancedPrompts"] == json!(true) {
        if let Some(chat) = crate::agent_chat::recent(&ctx.title) {
            system.push_str("
ADVANCED PROMPTING: below is the end of the speaker's recent chat with their AI coding agent, and the files it worked on. Use it only to understand what the speaker refers to (\"that button\", \"the error\", \"the same file\") and to name those things and files precisely in the prompt. Never repeat it, answer it, or add requests the speaker didn't say.
<recent_chat>
");
            system.push_str(&chat.replace("</recent_chat>", ""));
            system.push_str("
</recent_chat>");
        }
    }
    if cfg["dictation"]["speechSupport"] == json!(true) {
        system.push_str("\nSpeech support is on. The speaker may stutter, speak slowly, or have a speech difference or disability, so the transcript can hold repeated sounds, broken-off words, long pauses and sound-alike mistakes. Remove stutters and repeated starts. When a word is clearly a mishearing of a word the rest of the sentence makes obvious, write the intended word. Never guess beyond that, never change the meaning, and never add anything the speaker did not say.");
    }
    let mut examples = shots(wants_list, ctx.kind == "email");
    if better {
        examples.push(crate::prompt::better_shot());
    }
    Request { system, examples, better, rules }
}

// A model on this computer spends most of its time reading the prompt. Sending it while the person is still talking
// lets the model cache it, so formatting afterwards only reads the transcript. Cloud models are never warmed this way.
pub fn warm(ctx: &Context) {
    let (cfg, _, has_model) = snapshot();
    if !has_model || cfg["formatting"]["useAI"] == json!(false) || ctx.intensity <= 0 {
        return;
    }
    let cand = {
        let s = core::get().store.lock().unwrap();
        let best = route::rank_chat(&s, 0, 300.0, 100.0).into_iter().next();
        let local = best.as_ref().and_then(|r| s.find_model(r)).is_some_and(|(p, _)| p["local"] == json!(true));
        best.filter(|_| local).and_then(|r| hub::cands(&s, &[r], "chat").into_iter().next())
    };
    let Some(cand) = cand else { return };
    let Request { system, examples, .. } = compose(&cfg, ctx, &[], false, false);
    tokio::spawn(async move {
        let _ = crate::handle(json!({
            "id": 0, "op": "chat", "cands": [cand], "system": system, "user": wrap_user_ctx("", ""),
            "shots": crate::prompt::shots_json(&examples), "max_out": 1, "hedge_ms": 60000, "timeout_ms": 60000,
        }))
        .await;
    });
}

pub async fn format(raw: &str, ctx: &Context, local_only: bool) -> FormatOut {
    let (cfg, personal, has_model) = snapshot();
    let fixed;
    let raw = if !local_only && has_model && cfg["formatting"]["useAI"] != json!(false) {
        fixed = crate::vocab_ai::fix(raw).await;
        fixed.as_str()
    } else {
        raw
    };
    let p = prep(&cfg, has_model, &personal, raw, ctx);
    let (fmt, cleaned, verdict, ctx) = (&cfg["formatting"], &p.cleaned, &p.verdict, &p.ctx);
    let mut out = FormatOut {
        local_text: cleaned.text.clone(),
        text: cleaned.text.clone(),
        route: "local".into(),
        ..Default::default()
    };
    if cleaned.text.is_empty() {
        return out;
    }
    out.reason = verdict.reason.to_string();
    if local_only {
        out.reason = "Inserted as spoken (local)".into();
        return out;
    }
    if !verdict.ai {
        return out;
    }

    let privacy = &cfg["privacy"];
    let said = cleaned.text.to_lowercase();
    let vocabulary = if privacy["sendVocabulary"].as_bool().unwrap_or(true) {
        hint_terms(&personal, 60).into_iter().filter(|t| said.contains(&t.to_lowercase())).collect()
    } else {
        vec![]
    };
    let wants_list = cleaned.list_done || cleaned.signals.list_cue;
    let before = if cfg["dictation"]["messageContext"] == json!(true) { ctx.before.trim_end() } else { "" };
    let body = if wants_list {
        let joined = p.raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if ctx.kind != "code" && fmt["spokenCommands"] != json!(false) { text::local::spoken_emoji(&joined).0 } else { joined }
    } else {
        cleaned.text.clone()
    };
    let excluded = strings(&fmt["excludedChars"]);
    let Request { system, examples, better, rules } = compose(&cfg, ctx, &vocabulary, wants_list, !before.is_empty());
    let user = wrap_user_ctx(&body, before);
    let tin = estimate_tokens(&system)
        + estimate_tokens(&user)
        + examples.iter().map(|(u, a)| estimate_tokens(u) + estimate_tokens(a)).sum::<u64>();
    let tout = ((estimate_tokens(&body) as f64) * if better { 3.0 } else { 1.2 }).ceil() as u64 + 12;
    let refs = {
        let s = core::get().store.lock().unwrap();
        route::rank_chat(&s, verdict.tier, tin as f64, tout as f64)
    };
    if refs.is_empty() {
        out.reason = "No usable formatting model".into();
        return out;
    }

    let key = key_of(&[&system, &body, &refs.join(",")]);
    if let Some(hit) = CACHE.lock().unwrap().get(&key).cloned() {
        out.text = hit;
        out.route = "cache".into();
        out.reason = "Cached".into();
        return out;
    }

    let max_out = 8192u64.min(128u64.max(tout * 2 + 64)) as u32;
    match hub::chat(&refs, &system, &user, &examples, max_out).await {
        Ok(r) => {
            out.cost = r.cost;
            out.r = Some(r.r.clone());
            out.tokens_in = r.tokens_in;
            out.tokens_out = r.tokens_out;
            let concise = ctx.style["id"] == "concise";
            let checked = if better {
                crate::prompt::sanitize_prompt(&r.text, &body)
            } else {
                // a style or rule that rewrites words (abbreviations, slang) uses words the speaker never said
                let restyled = !ctx.instruction.trim().is_empty() || !rules.is_empty() || ctx.style["rewrites"] == json!(true);
                sanitize(&r.text, &body, concise || restyled, wants_list || ctx.intensity >= 3 || concise || restyled)
            };
            let Some(mut t) = checked else {
                out.reason = "AI reply rejected; used local formatting".into();
                return out;
            };
            t = drop_unsaid_terms(&t, &body, &personal);
            if !before.is_empty() {
                t = drop_context_lines(&t, before, &body);
            }
            if ctx.kind != "code" && fmt["spokenCommands"] != json!(false) {
                t = text::local::spoken_emoji(&t).0;
            }
            if ctx.kind != "code" {
                t = smart::normalize_lists(&t);
                t = smart::polish_punctuation(&t);
            }
            if ctx.kind != "code" {
                t = smart::humanize(&t, fmt["humanize"].as_str().unwrap_or("off"), p.formalize);
            }
            t = smart::exclude_chars(&text::replace_lit(crate::re!(r"[ \t]+\n"), &t, "\n"), &excluded)
                .trim()
                .to_string();
            {
                let mut c = CACHE.lock().unwrap();
                if c.len() > 200 {
                    c.clear();
                }
                c.insert(key, t.clone());
            }
            out.text = t;
            out.route = if verdict.tier >= 2 { "ai-complex" } else { "ai" }.into();
            out
        }
        Err(e) => {
            out.reason = format!("AI unavailable ({}); used local formatting", e.message);
            out.ai_error = Some(e.message);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_copied_from_the_box_are_dropped() {
        let before = "The installer is rebuilding. It reads the text already in the box. ";
        let said = "add a feature that reads the box and keep and lowercase";
        let out = "Add a feature that reads the box.

1. Keep and lowercase.

Context:
- It reads the text already in the box.

Don't:
- Capitalize and.";
        assert_eq!(drop_context_lines(out, before, said), "Add a feature that reads the box.

1. Keep and lowercase.

Don't:
- Capitalize and.");
        // what was said stays, even when the box has it too
        assert_eq!(drop_context_lines("It reads the text already in the box.", before, "it reads the text already in the box"), "It reads the text already in the box.");
    }
    fn ctx(kind: &str) -> Context {
        Context {
            app_name: String::new(),
            kind: kind.into(),
            style: json!({ "id": "natural", "name": "Natural", "instruction": "" }),
            intensity: 2,
            instruction: String::new(),
            forced: false,
            profile_id: None,
            before: String::new(),
            title: String::new(),
        }
    }

    #[test]
    fn words_phrases_and_characters_can_all_be_avoided() {
        let ex = |t: &str, list: &[&str]| {
            smart::exclude_chars(t, &list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        };
        assert_eq!(
            ex("Honestly, I think we should utilize the tool. It is great!", &["honestly", "utilize", "!"]),
            "I think we should the tool. It is great."
        );
        assert_eq!(
            ex("I hope this finds you well. Here is the report.", &["I hope this finds you well"]),
            "Here is the report."
        );
        assert_eq!(
            ex("The cat sat. Category five.", &["cat"]),
            "The sat. Category five.",
            "whole words only"
        );
        assert_eq!(ex("Fast \u{2014} and cheap", &["\u{2014}"]), "Fast, and cheap");
    }

    #[test]
    fn naming_three_or_more_things_makes_a_list_without_asking() {
        let e = |t: &str| smart::enumerate(t);
        assert_eq!(
            e("I'm gonna need this, that, and the other thing.").as_deref(),
            Some("I'm gonna need:\n- This\n- That\n- The other thing")
        );
        assert_eq!(
            e("For the trip I need my passport, the charger, some snacks and a jacket.").as_deref(),
            Some("For the trip I need:\n- My passport\n- The charger\n- Some snacks\n- A jacket")
        );
        assert_eq!(e("Milk, eggs, bread and coffee.").as_deref(), Some("- Milk\n- Eggs\n- Bread\n- Coffee"));
        assert_eq!(
            e("Quick update. Bring the laptop, the slides and the clicker. See you at 9.").as_deref(),
            Some("Quick update.\n\nBring:\n- The laptop\n- The slides\n- The clicker\n\nSee you at 9.")
        );
        for not_a_list in [
            "We went to the store, bought some milk, and then we drove all the way home.",
            "Can you grab milk, eggs and bread?",
            "Hi Sam, Maria and Alex.",
            "I called him, but he did not answer.",
            "Red and blue.",
        ] {
            assert_eq!(e(not_a_list), None, "{not_a_list}");
        }
        assert!(
            smart::has_enumeration("I need milk, eggs and bread.") && !smart::has_enumeration("I need milk.")
        );
    }

    #[test]
    fn auto_detect_lists_things_named_with_only_commas() {
        let e = |t: &str| smart::enumerate(t);
        assert_eq!(e("An apple, orange, grape.").as_deref(), Some("- An apple\n- Orange\n- Grape"));
        assert_eq!(
            e("I need an apple, orange, grape").as_deref(),
            Some("I need:\n- An apple\n- Orange\n- Grape")
        );
        for sentence in [
            "Yesterday, after lunch, we went home.",
            "Well, I think, maybe we should go.",
            "So, yeah, that works.",
        ] {
            assert_eq!(e(sentence), None, "{sentence}");
        }
    }

    #[test]
    fn rules_become_a_numbered_list_and_switched_off_ones_are_skipped() {
        let f = json!({ "rules": [{ "text": "Use British spelling.", "enabled": true }, { "text": "Never use exclamation marks." }, { "text": "Skip me", "enabled": false }, { "text": "  " }] });
        assert_eq!(rules_text(&f), "1. Use British spelling.\n2. Never use exclamation marks.");
        assert_eq!(rules_text(&json!({})), "");
    }

    #[test]
    fn punctuation_polish_fixes_commas_without_changing_words() {
        let p = smart::polish_punctuation;
        assert_eq!(p("Pay by card, and/or cash."), "Pay by card and/or cash.");
        assert_eq!(p("Pay by card and or cash."), "Pay by card and/or cash.");
        assert_eq!(
            p("We can either ship it, or you can collect it."),
            "We can either ship it or you can collect it."
        );
        assert_eq!(p("I went to the, store."), "I went to the store.");
        assert_eq!(
            p("We booked the room and we sent the invites."),
            "We booked the room, and we sent the invites."
        );
        assert_eq!(p("We booked the room and sent the invites."), "We booked the room and sent the invites.");
        assert_eq!(p("Salt and pepper."), "Salt and pepper.");
        assert_eq!(p("Hello,, there ."), "Hello, there.");
        assert_eq!(p("- milk and eggs\n- bread"), "- milk and eggs\n- bread");
    }

    #[test]
    fn a_vocabulary_word_the_model_added_is_removed_but_a_spoken_one_stays() {
        let p = json!({ "vocabulary": [{ "id": "1", "term": "AuraLink", "sounds": [] }, { "id": "2", "term": "OpenRouter", "sounds": [] }] });
        assert_eq!(
            drop_unsaid_terms("Hello, OpenRouter, AuraLink.", "hello OpenRouter", &p),
            "Hello, OpenRouter."
        );
        assert_eq!(drop_unsaid_terms("Hello AuraLink.", "hello AuraLink", &p), "Hello AuraLink.");
    }

    #[test]
    fn a_spoken_correction_is_applied_in_every_kind_of_app() {
        let cfg = crate::store::defaults();
        let p = json!({ "vocabulary": [] });
        let said = "see you at 5, no wait, 6";
        for kind in ["chat", "code", "prompt"] {
            assert!(
                prep(&cfg, false, &p, said, &ctx(kind)).cleaned.text.to_lowercase().starts_with("see you at 6"),
                "{kind}"
            );
        }
    }
}
