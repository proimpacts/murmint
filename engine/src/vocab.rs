use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

pub const MIN_KEY: usize = 5;

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub term: String,
    pub sounds: Vec<String>,
    pub note: String,
    pub trained: bool,
}

pub fn entries(personal: &Value) -> Vec<Entry> {
    personal["vocabulary"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| {
                    let term = v["term"].as_str()?.to_string();
                    Some(Entry {
                        term,
                        sounds: v["sounds"]
                            .as_array()
                            .map(|s| s.iter().filter_map(|x| x.as_str().map(|x| x.to_string())).collect())
                            .unwrap_or_default(),
                        note: v["note"].as_str().unwrap_or("").to_string(),
                        trained: v["trained"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn phonetic_key(s: &str) -> String {
    let t: String = s
        .to_lowercase()
        .nfd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .filter(|c| c.is_ascii_lowercase())
        .collect();
    if t.is_empty() {
        return String::new();
    }
    let mut t = t.replace("ph", "f").replace("ck", "k").replace("gh", "");
    for (a, b) in [("kn", "n"), ("wr", "r"), ("ps", "s")] {
        if t.starts_with(a) {
            t = format!("{b}{}", &t[a.len()..]);
        }
    }
    let chars: Vec<char> = t.chars().collect();
    let mut u = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == 'c' {
            u.push(if chars.get(i + 1).is_some_and(|n| "eiy".contains(*n)) { 's' } else { 'k' });
        } else {
            u.push(c);
        }
    }
    let u = u
        .replace('q', "k")
        .replace('x', "ks")
        .replace('z', "s")
        .replace("sch", "sk")
        .replace("sh", "x")
        .replace("ch", "x")
        .replace("th", "t");
    let u: String = u.chars().filter(|c| !"aeiouyhw".contains(*c)).collect();
    let mut out = String::new();
    for ch in u.chars() {
        let g = match ch {
            'b' | 'p' => 'p',
            'f' | 'v' => 'f',
            'g' | 'j' | 'k' => 'k',
            's' => 's',
            'd' | 't' => 't',
            'l' => 'l',
            'm' | 'n' => 'n',
            'r' => 'r',
            'x' => 'x',
            o => o,
        };
        if !out.ends_with(g) {
            out.push(g);
        }
    }
    out
}

pub fn letters_of(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

pub fn entry_keys(v: &Entry) -> Vec<String> {
    let mut keys: Vec<String> = vec![];
    let mut add = |k: String| {
        if k.len() >= MIN_KEY && !keys.contains(&k) {
            keys.push(k);
        }
    };
    add(phonetic_key(&v.term));
    for s in &v.sounds {
        add(phonetic_key(s));
    }
    keys
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

pub fn fuzzy_replace(text: &str, vocabulary: &[Entry]) -> String {
    struct E {
        term: String,
        keys: Vec<String>,
        len: usize,
    }
    let entries: Vec<E> = vocabulary
        .iter()
        .map(|v| E { term: v.term.clone(), keys: entry_keys(v), len: letters_of(&v.term).len() })
        .filter(|e| !e.keys.is_empty())
        .collect();
    if entries.is_empty() {
        return text.to_string();
    }
    let mut parts: Vec<String> = vec![];
    let (mut cur, mut in_ws) = (String::new(), false);
    for c in text.chars() {
        if c.is_whitespace() != in_ws {
            parts.push(std::mem::take(&mut cur));
            in_ws = !in_ws;
        }
        cur.push(c);
    }
    parts.push(cur);
    if !parts.is_empty() && text.chars().next().is_some_and(|c| c.is_whitespace()) {}
    let idx: Vec<usize> =
        parts.iter().enumerate().filter(|(i, p)| i % 2 == 0 && !p.is_empty()).map(|(i, _)| i).collect();
    let mut out = String::new();
    let mut w = 0;
    if text.chars().next().is_some_and(|c| c.is_whitespace()) {}
    if let Some(&first) = idx.first() {
        for p in &parts[..first] {
            out.push_str(p);
        }
    }
    while w < idx.len() {
        let mut hit: Option<(usize, String)> = None;
        let mut size = 3.min(idx.len() - w);
        while size >= 1 && hit.is_none() {
            let slice: Vec<&String> = idx[w..w + size].iter().map(|i| &parts[*i]).collect();
            let lead: String = slice[0].chars().take_while(|c| !is_word_char(*c)).collect();
            let tail_rev: String = slice[size - 1].chars().rev().take_while(|c| !is_word_char(*c)).collect();
            let tail: String = tail_rev.chars().rev().collect();
            let words: Vec<String> = slice
                .iter()
                .enumerate()
                .map(|(k, x)| {
                    let mut x = x.to_string();
                    if k == 0 {
                        x = x[lead.len().min(x.len())..].to_string();
                    }
                    if k == size - 1 && x.len() >= tail.len() {
                        x.truncate(x.len() - tail.len());
                    }
                    x
                })
                .collect();
            let joined = words.join(" ");
            let key = phonetic_key(&joined);
            if key.len() >= MIN_KEY {
                let letters = letters_of(&joined).len();
                for e in &entries {
                    if joined == e.term {
                        continue;
                    }
                    if !e.keys.contains(&key) {
                        continue;
                    }
                    if (letters as f64) < e.len as f64 * 0.6 || (letters as f64) > e.len as f64 * 1.8 {
                        continue;
                    }
                    hit = Some((size, format!("{lead}{}{tail}", e.term)));
                    break;
                }
            }
            size -= 1;
        }
        if let Some((size, replacement)) = hit {
            out.push_str(&replacement);
            let last = idx[w + size - 1];
            if let Some(&next) = idx.get(w + size) {
                for p in &parts[last + 1..next] {
                    out.push_str(p);
                }
            } else {
                for p in &parts[last + 1..] {
                    out.push_str(p);
                }
            }
            w += size;
        } else {
            out.push_str(&parts[idx[w]]);
            if let Some(&next) = idx.get(w + 1) {
                for p in &parts[idx[w] + 1..next] {
                    out.push_str(p);
                }
            } else {
                for p in &parts[idx[w] + 1..] {
                    out.push_str(p);
                }
            }
            w += 1;
        }
    }
    out
}

pub fn clean_heard(heard: &str, term: &str) -> Option<(bool, Option<String>)> {
    let trim_start = |c: char| c.is_whitespace() || "\"'“”‘’".contains(c);
    let trim_end = |c: char| c.is_whitespace() || "\"'“”‘’.,!?;:…".contains(c);
    let h = heard
        .trim_start_matches(trim_start)
        .trim_end_matches(trim_end)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if h.is_empty() {
        return None;
    }
    if h.split(' ').count() > 6 {
        return None;
    }
    if letters_of(&h) == letters_of(term) {
        return Some((true, None));
    }
    Some((false, Some(h.to_lowercase())))
}

#[allow(dead_code)]
pub fn merge_sounds(existing: &[String], variants: &[String], term: &str) -> Vec<String> {
    let mut seen: Vec<String> = vec![letters_of(term)];
    seen.extend(existing.iter().map(|s| letters_of(s)));
    let mut out: Vec<String> = existing.to_vec();
    for v in variants {
        let k = letters_of(v);
        if !k.is_empty() && !seen.contains(&k) {
            seen.push(k);
            out.push(v.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(term: &str, sounds: &[&str]) -> Entry {
        Entry {
            term: term.into(),
            sounds: sounds.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn words_that_sound_alike_share_a_key() {
        assert_eq!(phonetic_key("Kubernetes"), phonetic_key("cooper nettys"));
        assert_eq!(phonetic_key("Kubernetes"), phonetic_key("kuber netties"));
        assert_eq!(phonetic_key("Photograph"), phonetic_key("fotograf"));
        assert_ne!(phonetic_key("Kubernetes"), phonetic_key("Terraform"));
        assert_eq!(phonetic_key(""), "");
    }

    #[test]
    fn mishearings_map_back_and_punctuation_is_kept() {
        let voc = vec![v("Kubernetes", &["cooper nettys"]), v("Terraform", &[])];
        let f = |t: &str| fuzzy_replace(t, &voc);
        assert_eq!(f("We deploy to cooper netties every day."), "We deploy to Kubernetes every day.");
        assert_eq!(f("Roll it out on kuber netties, then check."), "Roll it out on Kubernetes, then check.");
        assert_eq!(f("we use terra form for that"), "we use Terraform for that");
        assert_eq!(f("We deploy to Kubernetes already."), "We deploy to Kubernetes already.");
        assert_eq!(f("The cat sat on the mat."), "The cat sat on the mat.");
        assert_eq!(f("  leading space"), "  leading space");
    }

    #[test]
    fn short_names_are_only_matched_exactly() {
        let voc = vec![v("Sam", &[])];
        assert_eq!(fuzzy_replace("some same seem sum", &voc), "some same seem sum");
    }

    #[test]
    fn training_takes() {
        assert_eq!(
            clean_heard(" Cooper Nettys. ", "Kubernetes"),
            Some((false, Some("cooper nettys".into())))
        );
        assert_eq!(clean_heard("Kubernetes.", "Kubernetes"), Some((true, None)));
        assert_eq!(clean_heard("", "x"), None);
        assert_eq!(
            clean_heard("this is a long sentence that is not just the word at all", "Kubernetes"),
            None
        );
        assert_eq!(
            merge_sounds(
                &["cooper nettys".into()],
                &["Cooper Nettys".into(), "kuber netties".into(), "Kubernetes".into()],
                "Kubernetes"
            ),
            vec!["cooper nettys", "kuber netties"]
        );
    }
}
