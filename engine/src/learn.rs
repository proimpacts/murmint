use crate::re;
use crate::text::{grp, replace_all, replace_lit, Regex};
use crate::vocab::{self, Entry};
use serde_json::Value;

fn rank(e: &Entry) -> u8 {
    if e.trained {
        return 0;
    }
    match e.note.as_str() {
        "trained" => 0,
        "edit" => 1,
        "voice" => 2,
        "manual" => 3,
        "auto" => 4,
        _ => 3,
    }
}

pub fn hint_terms(personal: &Value, max: usize) -> Vec<String> {
    let mut list: Vec<(u8, usize, String)> =
        vocab::entries(personal).into_iter().enumerate().map(|(i, e)| (rank(&e), i, e.term)).collect();
    list.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    list.into_iter().take(max).map(|x| x.2).collect()
}

pub fn stt_prompt(personal: &Value) -> String {
    let terms = hint_terms(personal, 15);
    if terms.is_empty() {
        return String::new();
    }
    let mut out = String::from("Vocabulary: ");
    for t in terms {
        if out.chars().count() + t.chars().count() > 240 {
            break;
        }
        out.push_str(&t);
        out.push_str(", ");
    }
    out.trim_end_matches(", ").to_string() + "."
}

pub fn strip_vocab_echo(text: &str, personal: &Value) -> String {
    let original = text.to_string();
    let voc = vocab::entries(personal);
    let mut terms: Vec<String> =
        voc.iter().map(|v| v.term.clone()).filter(|t| t.chars().count() > 1).collect();
    terms.sort();
    terms.dedup();
    let mut t = replace_lit(re!(r"(?i)\bvocabulary\s*:\s*"), &original, "");
    if terms.len() >= 3 {
        terms.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
        let alt = terms.iter().map(|x| fancy_regex::escape(x).to_string()).collect::<Vec<_>>().join("|");
        if let (Ok(run), Ok(one)) = (
            Regex::new(&format!(r"(?i)(?:(?:^|[\s,;.]+)(?:{alt})(?=$|[\s,;.])){{3,}}[\s,;.]*")),
            Regex::new(&format!("(?i){alt}")),
        ) {
            t = replace_all(&run, &t, |c| {
                let m = grp(c, 0);
                let mut seen: Vec<String> =
                    one.find_iter(m).flatten().map(|x| x.as_str().to_lowercase()).collect();
                seen.sort();
                seen.dedup();
                if seen.len() >= 2 {
                    " ".to_string()
                } else {
                    m.to_string()
                }
            });
        }
    }
    let removed = t != original;
    if removed {
        t = replace_lit(re!(r"[ \t]{2,}"), &t, " ");
        t = replace_all(re!(r"[ \t]+([,.;!?])"), &t, |c| grp(c, 1).to_string()).trim().to_string();
    }
    let words: Vec<String> = re!(r"[\p{L}\p{N}']+")
        .find_iter(&t.to_lowercase())
        .flatten()
        .map(|m| m.as_str().to_string())
        .collect();
    if !words.is_empty() && words.len() <= 40 {
        let mut known: Vec<String> = vec!["vocabulary".into()];
        for v in &voc {
            for m in re!(r"[\p{L}\p{N}']+").find_iter(&v.term.to_lowercase()).flatten() {
                known.push(m.as_str().to_string());
            }
        }
        let mut distinct = words.clone();
        distinct.sort();
        distinct.dedup();
        if words.len() >= 3
            && voc.len() >= 2
            && words.iter().all(|w| known.contains(w))
            && distinct.len() >= 2
        {
            return String::new();
        }
        if words.len() == 1 && words[0] == "vocabulary" {
            return String::new();
        }
    }
    if removed {
        t
    } else {
        original
    }
}

pub fn parse_vocab_command(raw: &str) -> Option<String> {
    let t = re!(r"[.!?]+$").replace(raw.trim(), "").to_string();
    // "add Kubernetes to my vocabulary", "add to vocabulary: Kubernetes", "vocab: Kubernetes"
    let c = re!(
        r#"(?i)^(?:please\s+)?(?:add|save|remember|learn|put)\s+(?:the\s+)?(?:(?:word|name|term|spelling)\s+)?["“']?(.+?)["”']?\s+(?:to|in|into|as)\s+(?:my\s+|the\s+)?(?:vocabulary|dictionary|vocab)$"#
    )
    .captures(&t)
    .ok()
    .flatten()
    .or_else(|| {
        re!(
            r#"(?i)^(?:add|save)\s+(?:to|in|into)\s+(?:my\s+|the\s+)?(?:vocabulary|dictionary|vocab)[:,]?\s+["“']?(.+?)["”']?$"#
        )
        .captures(&t)
        .ok()
        .flatten()
    })
    .or_else(|| re!(r#"(?i)^(?:vocabulary|vocab)[:,]\s+["“']?(.+?)["”']?$"#).captures(&t).ok().flatten())?;
    let term = grp(&c, 1).trim().to_string();
    if !term.is_empty() && term.split_whitespace().count() <= 4 {
        Some(term)
    } else {
        None
    }
}

// ---------- vocabulary suggestions ----------
// Murmint notices unusual words in what you dictate: names, brands and terms written in a way ordinary words are not
// (OpenRouter, iPhone, GPT5, NASA, or a capitalised word in the middle of a sentence). Once one has turned up in
// SUGGEST_AFTER different sentences, the Vocabulary page offers it. Nothing is added without a click.

pub const SUGGEST_AFTER: usize = 3;
const KEEP_WORDS: usize = 200;
const KEEP_SENTENCES: usize = 3;

// capitalised words that are ordinary English, not names worth learning
const ORDINARY: &[&str] = &[
    "i", "i'm", "i'd", "i'll", "i've", "ok", "okay", "am", "pm", "tv", "usa", "uk", "us", "eu", "faq", "id", "pdf", "url",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january", "february", "march",
    "april", "may", "june", "july", "august", "september", "october", "november", "december", "english", "god",
    "mr", "mrs", "ms", "dr", "christmas", "easter", "internet", "google", "windows", "microsoft", "apple", "discord",
];

fn unusual(word: &str, starts_sentence: bool) -> bool {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric());
    if w.chars().count() < 3 || ORDINARY.contains(&w.to_lowercase().as_str()) {
        return false;
    }
    let letters = w.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 2 {
        return false;
    }
    let digits = w.chars().any(|c| c.is_ascii_digit());
    let upper_inside = w.chars().skip(1).any(|c| c.is_uppercase()) && w.chars().any(|c| c.is_lowercase());
    let all_caps = w.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && letters <= 6;
    let named = !starts_sentence && w.chars().next().is_some_and(|c| c.is_uppercase());
    digits || upper_inside || all_caps || named
}

/// The unusual words in a piece of text, each with the sentence it was in.
pub fn rare_words(text: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    for sentence in crate::text::smart::split_sentences(text) {
        for line in sentence.lines() {
            let line = line.trim_start_matches(|c: char| c == '-' || c == '*' || c.is_ascii_digit() || c == '.' || c.is_whitespace());
            for (i, raw) in line.split_whitespace().enumerate() {
                if unusual(raw, i == 0) {
                    let w = raw.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
                    if !out.iter().any(|(x, _): &(String, String)| x.eq_ignore_ascii_case(&w)) {
                        out.push((w, sentence.chars().take(140).collect()));
                    }
                }
            }
        }
    }
    out
}

/// Notes the unusual words in a finished dictation. Returns true when the saved state changed.
pub fn notice(personal: &mut Value, text: &str) -> bool {
    let known: Vec<String> = vocab::entries(personal).iter().map(|e| e.term.to_lowercase()).collect();
    let dismissed: Vec<String> = personal["vocabDismissed"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_lowercase())).collect())
        .unwrap_or_default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    if !personal["vocabSeen"].is_object() {
        personal["vocabSeen"] = serde_json::json!({});
    }
    let mut changed = false;
    for (word, sentence) in rare_words(text) {
        let key = word.to_lowercase();
        if known.contains(&key) || dismissed.contains(&key) {
            continue;
        }
        let seen = &mut personal["vocabSeen"][&key];
        if !seen.is_object() {
            *seen = serde_json::json!({ "term": word, "sentences": [], "last": 0 });
        }
        let list = seen["sentences"].as_array_mut().unwrap();
        // the same sentence said twice counts once
        if !list.iter().any(|s| s.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&sentence))) {
            list.push(Value::String(sentence));
            if list.len() > KEEP_SENTENCES {
                list.remove(0);
            }
            seen["count"] = serde_json::json!(seen["count"].as_u64().unwrap_or(0) + 1);
        }
        seen["last"] = serde_json::json!(now);
        changed = true;
    }
    // keep only the most recent words
    if let Some(map) = personal["vocabSeen"].as_object_mut() {
        if map.len() > KEEP_WORDS {
            let mut by_age: Vec<(String, u64)> = map.iter().map(|(k, v)| (k.clone(), v["last"].as_u64().unwrap_or(0))).collect();
            by_age.sort_by_key(|x| x.1);
            for (k, _) in by_age.into_iter().take(map.len() - KEEP_WORDS) {
                map.remove(&k);
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn personal(terms: &[&str]) -> Value {
        let vocabulary: Vec<Value> = terms
            .iter()
            .enumerate()
            .map(|(i, t)| json!({ "id": i.to_string(), "term": t, "sounds": [] }))
            .collect();
        json!({ "vocabulary": vocabulary })
    }

    #[test]
    fn the_hint_is_small_and_recorded_words_come_first() {
        let mut v: Vec<Value> = (0..80)
            .map(
                |i| json!({ "id": i.to_string(), "term": format!("Auto{i}x"), "sounds": [], "note": "auto" }),
            )
            .collect();
        v.push(json!({ "id": "r", "term": "Kubernetes", "sounds": [], "note": "trained", "trained": true }));
        let p = json!({ "vocabulary": v });
        let h = stt_prompt(&p);
        assert!(h.len() <= 260 && h.starts_with("Vocabulary: Kubernetes"), "{h}");
        assert_eq!(hint_terms(&p, 15).len(), 15);
        assert_eq!(stt_prompt(&json!({})), "");
    }

    #[test]
    fn a_read_back_word_list_is_removed_and_real_sentences_are_not() {
        let p = personal(&["Kubernetes", "Siobhan", "Terraform", "FastAPI", "Bluefin"]);
        let s = |t: &str| strip_vocab_echo(t, &p);
        assert_eq!(s("Vocabulary: Kubernetes, Siobhan, Terraform, FastAPI, Bluefin."), "");
        assert_eq!(
            s("Kubernetes, Siobhan, Terraform, FastAPI, Bluefin. We should deploy it today."),
            "We should deploy it today."
        );
        assert_eq!(
            s("Deploy it with Terraform and Kubernetes today."),
            "Deploy it with Terraform and Kubernetes today."
        );
        assert_eq!(
            s("Ask Siobhan about Kubernetes and FastAPI."),
            "Ask Siobhan about Kubernetes and FastAPI."
        );
        assert_eq!(s("hello there"), "hello there");
        assert_eq!(
            strip_vocab_echo("Kubernetes Siobhan Terraform", &json!({ "vocabulary": [] })),
            "Kubernetes Siobhan Terraform"
        );
        let wild = personal(&["Dead", "Trilogy", "Rockstar", "Revolver", "Rust"]);
        assert_eq!(strip_vocab_echo("Vocabulary: Dead, Dead, Trilogy, Rockstar, Revolver, Dead, Dead, Trilogy, Rockstar, Rust, Vocabulary.", &wild), "");
        assert_eq!(
            strip_vocab_echo("Dead, Dead, Trilogy, Rockstar, Revolver. we should go", &wild),
            "we should go"
        );
        let mail = "Hi Sam,\n\nThanks for the update.\n\nThanks,\nAlex";
        assert_eq!(strip_vocab_echo(mail, &wild), mail, "line breaks survive");
    }

    #[test]
    fn spoken_vocabulary_commands() {
        assert_eq!(parse_vocab_command("add Kubernetes to my vocabulary."), Some("Kubernetes".into()));
        assert_eq!(
            parse_vocab_command("Please remember the word Siobhan in the dictionary"),
            Some("Siobhan".into())
        );
        assert_eq!(parse_vocab_command("vocab: Terraform"), Some("Terraform".into()));
        assert_eq!(parse_vocab_command("add milk to my shopping list"), None);
        assert_eq!(parse_vocab_command("add one two three four five to my vocabulary"), None);
    }

    #[test]
    fn unusual_words_are_suggested_after_three_sentences() {
        let words: Vec<String> = rare_words("I set up OpenRouter today. Then I asked Siobhan about GPT5 and the NASA deck.").into_iter().map(|x| x.0).collect();
        assert_eq!(words, ["OpenRouter", "Siobhan", "GPT5", "NASA"]);
        assert!(rare_words("Okay so I think we should go home on Monday.").is_empty(), "ordinary words are not suggested");
        let mut p = serde_json::json!({ "vocabulary": [] });
        notice(&mut p, "We use OpenRouter for this.");
        notice(&mut p, "We use OpenRouter for this.");
        assert_eq!(p["vocabSeen"]["openrouter"]["count"], 1, "the same sentence counts once");
        notice(&mut p, "OpenRouter is down again.");
        notice(&mut p, "Move it to OpenRouter tomorrow.");
        assert_eq!(p["vocabSeen"]["openrouter"]["count"], 3);
        let mut known = serde_json::json!({ "vocabulary": [{ "term": "OpenRouter" }], "vocabDismissed": ["siobhan"] });
        notice(&mut known, "OpenRouter and Siobhan.");
        assert!(known["vocabSeen"].as_object().unwrap().is_empty(), "words already known or dismissed are skipped");
    }
}
