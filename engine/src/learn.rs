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
}
