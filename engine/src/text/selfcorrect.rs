use super::*;
use crate::re;
use crate::vocab::{letters_of, phonetic_key};

fn is_match(re: &Regex, s: &str) -> bool {
    re.is_match(s).unwrap_or(false)
}

fn distance_ratio(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<char>, Vec<char>) = (letters_of(a).chars().collect(), letters_of(b).chars().collect());
    if a.is_empty() || b.is_empty() {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1])));
        }
        prev = cur;
    }
    prev[b.len()] as f64 / a.len().max(b.len()) as f64
}

fn alike(x: &str, y: &str) -> bool {
    let (lx, ly) = (letters_of(x), letters_of(y));
    if lx == ly {
        return false;
    }
    let (kx, ky) = (phonetic_key(x), phonetic_key(y));
    (!kx.is_empty() && kx == ky) || distance_ratio(x, y) <= 0.6
}

fn marker() -> &'static Regex {
    re!(
        r"(?i)(?:^|[\s,;.])(?:(?:ah|oh|um|uh|er|hmm),?\s+)*(no,? wait|wait,? no|no,? i meant|no,? i mean|oh,? wait|no,? no|actually,? no|actually,? make (?:it|that)|make (?:it|that)|or rather|i meant|you know what|correction|i mean|sorry|actually)\b[,.:;]?\s+"
    )
}
fn is_strong(m: &str) -> bool {
    let m = m.to_lowercase();
    !(m == "i mean" || m == "sorry" || m == "actually" || m == "you know what")
}

fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}
fn bare(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase()
}

// "no wait" / "I mean" corrections, within one dictation
pub fn apply(text: &str) -> (String, bool) {
    let mut t = text.to_string();
    let mut changed = false;
    // "8:30 no 9:30": a bare "no" between two times or numbers keeps the second
    let bare_no = re!(r"(?i)\b\d+(?::\d{2})?(?:\s?[ap]\.?m\.?)?,?\s+no,?\s+(\d+(?::\d{2})?(?:\s?[ap]\.?m\.?)?)");
    // "open beta release or open alpha release, actually": two options ending in the same word, then "actually",
    // keeps the second option
    let either = re!(r"(?i)\b((?:[\w'-]+\s+){0,3})([\w'-]+)\s+or\s+((?:[\w'-]+\s+){0,3})\2,?\s+actually\b,?\s*");
    let next = bare_no.replace_all(&t, "$1").to_string();
    // the first option is as long as the second; words before it stay ("it says open beta release or ...")
    let next = replace_all(either, &next, |c| {
        let second = grp(c, 3);
        let first: Vec<&str> = grp(c, 1).split_whitespace().collect();
        let keep_before = first.len().saturating_sub(second.split_whitespace().count());
        let before = first[..keep_before].join(" ");
        format!("{}{}{} ", before, if before.is_empty() { "" } else { " " }, format!("{second}{}", grp(c, 2)))
    });
    if next != t {
        t = next;
        changed = true;
    }
    for _ in 0..6 {
        let Some(found) = find_fix(&t) else { break };
        t = found;
        changed = true;
    }
    (t, changed)
}

fn find_fix(t: &str) -> Option<String> {
    let mut from = 0;
    while let Ok(Some(c)) = marker().captures(&t[from..]) {
        let m0 = c.get(0).unwrap();
        let (ms, me) = (from + m0.start(), from + m0.end());
        let kind = grp(&c, 1).to_string();
        let before = t[..ms].trim_end_matches(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace());
        let tail = &t[me..];
        if !before.is_empty() {
            if let Some(fixed) = fix(before, tail, &kind) {
                return Some(fixed);
            }
        }
        from = me;
    }
    None
}

fn fix(before: &str, tail: &str, kind: &str) -> Option<String> {
    let strong = is_strong(kind);
    let tw = words(tail);
    if tw.is_empty() {
        return None;
    }
    let new_sentence = is_match(re!(r"[.!?]\s*$"), before) && !is_match(re!(r"(?i)\b[ap]\.m\.\s*$"), before);
    let refers_back = is_match(re!(r"(?i)^(?:to\s+)?(?:say|write|type|send|put)\b"), tail.trim_start());

    if tw.len() >= 3 && !refers_back {
        let (w1, w2) = (bare(tw[0]), bare(tw[1]));
        let bw: Vec<(usize, &str)> = before
            .split_whitespace()
            .scan(0usize, |pos, w| {
                let start = before[*pos..].find(w).map(|i| i + *pos).unwrap();
                *pos = start + w.len();
                Some((start, w))
            })
            .collect();
        for i in (0..bw.len().saturating_sub(1)).rev() {
            if bare(bw[i].1) == w1 && bare(bw[i + 1].1) == w2 {
                let rest = strip_tail_filler(tail);
                return Some(format!("{}{}", &before[..bw[i].0], rest).trim().to_string());
            }
        }
    }

    if new_sentence || refers_back {
        return None;
    }

    let num = re!(&format!(
        r"(?i)(?:\d+(?::\d{{2}})?|\b(?:{n})(?:[\s-]+(?:{n}))*\b)(?:\s?(?:a\.?m\.?|p\.?m\.?))?",
        n = "zero|one|two|three|four|five|six|seven|eight|nine|ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen|twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety|hundred|thousand|million"
    ));
    if let Ok(Some(newn)) = num.find(tail) {
        let (lead, after) = (&tail[..newn.start()], &tail[newn.end()..]);
        // word number must lead, or "the other one" counts
        let in_words = !newn.as_str().starts_with(|c: char| c.is_ascii_digit());
        if words(lead).len() <= 2 && !is_match(re!(r"[.!?]"), lead) && (!in_words || lead.trim().is_empty()) {
            let last = num.find_iter(before).flatten().last();
            if let Some(old) = last {
                let rest =
                    strip_tail_filler(after.trim_start_matches(|c: char| c == ',' || c.is_whitespace()));
                let old_after = before[old.end()..].trim().to_lowercase();
                let joined = if rest.is_empty() || rest.to_lowercase() == old_after {
                    String::new()
                } else {
                    format!(" {rest}")
                };
                return Some(
                    format!("{}{}{}{}", &before[..old.start()], newn.as_str(), &before[old.end()..], joined)
                        .trim()
                        .to_string(),
                );
            }
        }
    }

    let bwords = words(before);
    for n in 1..=3usize {
        if tw.len() < n || bwords.len() < n {
            break;
        }
        let y: String = tw[..n].iter().map(|w| bare(w)).collect::<Vec<_>>().join(" ");
        let x: String = bwords[bwords.len() - n..].iter().map(|w| bare(w)).collect::<Vec<_>>().join(" ");
        if y.is_empty() || x.is_empty() {
            continue;
        }
        let word_by_word = x.split(' ').zip(y.split(' ')).all(|(a, b)| a == b || alike(a, b));
        let swap_ok = if strong {
            n <= 2 && !is_match(re!(r"[.!?]$"), tw[n - 1]) || (word_by_word && alike(&x, &y))
        } else {
            word_by_word && alike(&x, &y)
        };
        if !swap_ok || y == x {
            continue;
        }
        let keep = before[..before.len() - bwords[bwords.len() - n..].join(" ").len()].to_string();
        let y_text = tw[..n].join(" ");
        let y_text = y_text.trim_end_matches(|c: char| c == ',' || c == ';');
        let rest = tw[n..].join(" ");
        return Some(
            format!("{keep}{y_text}{}", if rest.is_empty() { String::new() } else { format!(" {rest}") })
                .trim()
                .to_string(),
        );
    }
    None
}

fn strip_tail_filler(s: &str) -> String {
    re!(r"(?i)[\s,]*\b(?:instead|rather|please|then|okay|ok)\s*([.!?]?)\s*$")
        .replace(s.trim(), "$1")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(s: &str) -> String {
        apply(s).0
    }

    #[test]
    fn a_time_or_number_is_replaced() {
        assert_eq!(f("let's meet at 8:30, ah you know what, let's meet at 9 instead"), "let's meet at 9");
        assert_eq!(f("see you at 5, no wait, 6"), "see you at 6");
        assert_eq!(f("it costs 40 dollars, I meant 50 dollars"), "it costs 50 dollars");
    }

    #[test]
    fn a_word_is_replaced_by_the_word_that_was_meant() {
        assert_eq!(f("take a snapshot, I mean screenshot of the page"), "take a screenshot of the page");
        assert_eq!(f("send it to Sam, no wait, Maria"), "send it to Maria");
        assert_eq!(f("we use Postgres, or rather MySQL for this"), "we use MySQL for this");
    }

    #[test]
    fn ordinary_uses_of_the_same_words_are_left_alone() {
        for s in [
            "I mean it is really fine",
            "sorry about that",
            "I actually like it",
            "you know what I think it works",
            "no wait is not what I said",
            "it was fine, I mean really fine today",
        ] {
            assert_eq!(f(s), s, "{s}");
        }
        assert_eq!(f("hello there"), "hello there");
        assert!(!apply("send the file").1);
    }

    #[test]
    fn a_correction_only_reaches_back_within_the_same_sentence() {
        for s in [
            "Sorry about the last message. I meant to say we meet at 9.",
            "I meant to say Tuesday, not Monday.",
            "Oops. I mean the blue one.",
            "That was a typo. Sorry, Maria.",
        ] {
            assert_eq!(f(s), s, "{s}");
        }
        assert_eq!(f("send it to Sam, no wait, Maria"), "send it to Maria");
        assert!(f("Let's meet at 8. No wait, let's meet at 9.").eq_ignore_ascii_case("let's meet at 9."));
    }

    #[test]
    fn numbers_said_in_words_are_corrected_too() {
        assert_eq!(f("lets meet at eight pm no wait nine pm"), "lets meet at nine pm");
        assert_eq!(f("it costs forty dollars, I meant fifty dollars"), "it costs fifty dollars");
        assert_eq!(f("bring two, no wait, three chairs"), "bring three chairs");
        assert_eq!(f("see you at five, no wait, six"), "see you at six");
        assert_eq!(f("I want one, no wait, the other one"), "I want the other one");
    }

    #[test]
    fn several_corrections_in_a_row_all_apply() {
        assert_eq!(f("meet at 8, no wait, 9, no wait, 10"), "meet at 10");
    }

    #[test]
    fn times_with_am_pm_are_corrected() {
        assert_eq!(f("let's meet at 8 p.m., no wait, 9 p.m."), "let's meet at 9 p.m.");
        assert_eq!(f("let's meet at 8 p.m. no I meant 9 p.m."), "let's meet at 9 p.m.");
        assert_eq!(f("let's meet at 8 PM, I meant 9 PM"), "let's meet at 9 PM");
        assert_eq!(f("see you at 8 p.m., actually make it 9"), "see you at 9");
        assert_eq!(
            f("dinner is at 7:30 p.m., sorry, 8:30 p.m. at the new place"),
            "dinner is at 8:30 p.m. at the new place"
        );
        assert_eq!(f("meet at 8, no I meant 9"), "meet at 9");
        assert_eq!(f("at 8 a.m., oh wait, I mean 9 a.m."), "at 9 a.m.");
    }
}

#[cfg(test)]
mod corrections_said_aloud {
    #[test]
    fn times_and_options_corrected_aloud_keep_only_the_fix() {
        for (said, want) in [
            ("let's meet at 8:30 I mean 9:30", "let's meet at 9:30"),
            ("let's meet at 8:30, actually 9:30", "let's meet at 9:30"),
            ("let's meet at 8:30 no 9:30 tomorrow", "let's meet at 9:30 tomorrow"),
            ("meet at 8:30 sorry 9:30", "meet at 9:30"),
            ("it says open beta release or open alpha release actually December 1st", "it says open alpha release December 1st"),
        ] {
            assert_eq!(super::apply(said).0, want, "{said}");
        }
        // a real either-or stays
        assert_eq!(super::apply("tea or coffee is fine").0, "tea or coffee is fine");
    }
}
