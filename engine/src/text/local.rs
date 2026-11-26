use super::smart;
use super::*;
use crate::re;
use crate::vocab;
use serde_json::Value;

fn is_match(re: &Regex, s: &str) -> bool {
    re.is_match(s).unwrap_or(false)
}

const NO_DEDUPE: &[&str] = &[
    "had", "that", "is", "very", "no", "so", "bye", "ha", "really", "knock", "now", "yes", "blah", "go",
    "well", "many", "much",
];
pub fn escape_re(s: &str) -> String {
    fancy_regex::escape(s).to_string()
}

fn phrase_regex(phrase: &str) -> Regex {
    let body = phrase.split_whitespace().map(escape_re).collect::<Vec<_>>().join(r"\s+");
    Regex::new(&format!(r"(?i)(?<![\w']){body}(?![\w'])")).unwrap_or_else(|_| Regex::new("$^").unwrap())
}

pub fn apply_personal(text: &str, personal: &Value) -> String {
    if personal.is_null() {
        return text.to_string();
    }
    let voc = vocab::entries(personal);
    let mut rules: Vec<(String, String)> = vec![];
    for v in &voc {
        for s in &v.sounds {
            if !s.trim().is_empty() && s.trim().to_lowercase() != v.term.to_lowercase() {
                rules.push((s.clone(), v.term.clone()));
            }
        }
    }
    rules.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));
    let mut out = text.to_string();
    for (from, to) in &rules {
        out = replace_lit(&phrase_regex(from), &out, to);
    }
    for v in &voc {
        if !v.term.is_empty() && is_match(re!(r"[A-Z]|\d|\."), &v.term) {
            out = replace_lit(&phrase_regex(&v.term), &out, &v.term);
        }
    }
    vocab::fuzzy_replace(&out, &voc)
}

// "thumbs up emoji" -> 👍. Most need "emoji"; a few faces also accept "face".
const EMOJI: &[(&str, &str)] = &[
    ("smiley", "🙂"),
    ("smiling", "😊"),
    ("happy", "😊"),
    ("grinning", "😀"),
    ("big smile", "😀"),
    ("laughing", "😂"),
    ("laugh", "😂"),
    ("crying laughing", "😂"),
    ("winking", "😉"),
    ("wink", "😉"),
    ("sad", "🙁"),
    ("frowny", "🙁"),
    ("crying", "😢"),
    ("angry", "😠"),
    ("surprised", "😮"),
    ("kissing", "😘"),
    ("blushing", "😊"),
    ("cool", "😎"),
    ("sunglasses", "😎"),
    ("thinking", "🤔"),
    ("eye roll", "🙄"),
    ("shrug", "🤷"),
    ("skull", "💀"),
    ("heart", "❤️"),
    ("red heart", "❤️"),
    ("broken heart", "💔"),
    ("thumbs up", "👍"),
    ("thumbs down", "👎"),
    ("clapping", "👏"),
    ("clap", "👏"),
    ("fire", "🔥"),
    ("party", "🎉"),
    ("party popper", "🎉"),
    ("praying", "🙏"),
    ("folded hands", "🙏"),
    ("hundred", "💯"),
    ("check mark", "✅"),
    ("star", "⭐"),
    ("sparkles", "✨"),
    ("sobbing", "😭"),
    ("poop", "💩"),
    ("eyes", "👀"),
    ("wave", "👋"),
    ("muscle", "💪"),
];
const FACE_WORD: &[&str] = &["smiley", "frowny", "winking", "laughing", "crying"];

fn emoji_for(name: &str) -> Option<&'static str> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    EMOJI.iter().find(|(n, _)| *n == name).map(|(_, e)| *e)
}

pub fn spoken_emoji(text: &str) -> (String, bool) {
    let mut used = false;
    let names: Vec<String> = EMOJI.iter().map(|(n, _)| n.replace(' ', r"\s+")).collect();
    let faces = FACE_WORD.join("|");
    let pattern = format!(
        r"(?i)(?:\b(?:a|an)\s+)?\b(?:({})\s+(?:face\s+)?emoji|({})\s+face)\b",
        names.join("|"),
        faces
    );
    let re = fancy_regex::Regex::new(&pattern).unwrap();
    let t = replace_all(&re, text, |c| {
        let name = if grp(c, 1).is_empty() { grp(c, 2) } else { grp(c, 1) };
        match emoji_for(name) {
            Some(e) => {
                used = true;
                e.to_string()
            }
            None => grp(c, 0).to_string(),
        }
    });
    (t, used)
}

/// "what time is it question mark" becomes "what time is it?". Any punctuation the transcript already put around
/// the spoken mark is absorbed, so the sentence never ends up with two. "period" is only taken at the very end,
/// where it can't be the noun.
pub fn spoken_punctuation(text: &str) -> (String, bool) {
    let mut used = false;
    let re = re!(
        r"(?i)\s*[.,!?;:]*\s*\b(?:(question mark)|(exclamation (?:mark|point))|(full stop)|(semicolon)|(colon)|(comma)|(period)\s*$)[.,!?;:]*"
    );
    let t = replace_all(re, text, |c| {
        used = true;
        let mark = if c.get(1).is_some() {
            "?"
        } else if c.get(2).is_some() {
            "!"
        } else if c.get(4).is_some() {
            ";"
        } else if c.get(5).is_some() {
            ":"
        } else if c.get(6).is_some() {
            ","
        } else {
            "."
        };
        mark.to_string()
    });
    (t, used)
}

pub fn remove_fillers(text: &str, aggressive: bool) -> String {
    let mut t =
        replace_all(re!(r"(?i)(,)?\s*\b(?:um+|uh+|uhm+|umm+|erm+|er|hmm+|mhm+)\b(?:\s*,)?[.]?"), text, |c| {
            if c.get(1).is_some() {
                ", ".to_string()
            } else {
                " ".to_string()
            }
        });
    if aggressive {
        t = replace_lit(re!(r"(?i)(?:^|(?<=[.!?]\s))(?:so|well|like|basically|actually),?\s+(?=\w)"), &t, "");
        t = replace_lit(re!(r"(?i),?\s*\b(?:you know|i mean|sort of|kind of)\b,"), &t, ",");
    }
    replace_lit(re!(r" {2,}"), &t, " ")
}

const NUMBER_WORDS: &[&str] = &[
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "sixty",
    "seventy",
    "eighty",
    "ninety",
    "hundred",
    "thousand",
    "million",
    "oh",
];

/// "five five five" is a real repeat
fn only_numbers(phrase: &str) -> bool {
    phrase.split_whitespace().all(|w| {
        NUMBER_WORDS.contains(&w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase().as_str())
    })
}

pub fn dedupe(text: &str) -> String {
    static RES: std::sync::LazyLock<[Regex; 2]> = std::sync::LazyLock::new(|| {
        [3usize, 2]
            .map(|n| Regex::new(&format!(r"(?i)\b((?:[\w']+\s+){{{}}}[\w']+)(?:,?\s+\1\b)+", n - 1)).unwrap())
    });
    let mut t = text.to_string();
    for re in RES.iter() {
        t = replace_all(re, &t, |c| {
            if only_numbers(grp(c, 1)) {
                grp(c, 0).to_string()
            } else {
                grp(c, 1).to_string()
            }
        });
    }
    replace_all(re!(r"(?i)\b([A-Za-z']+)(?:,?\s+\1\b)+"), &t, |c| {
        if NO_DEDUPE.contains(&grp(c, 1).to_lowercase().as_str()) || only_numbers(grp(c, 1)) {
            grp(c, 0).to_string()
        } else {
            grp(c, 1).to_string()
        }
    })
}

pub fn tidy(text: &str, capitalize: bool, english: bool) -> String {
    let mut t = text.replace('\r', "");
    t = replace_lit(re!(r"[ \t]+"), &t, " ");
    t = replace_all(re!(r"[ \t]+([,.;:!?%)\]}])"), &t, |c| grp(c, 1).to_string());
    t = replace_all(re!(r"([({\[])[ \t]+"), &t, |c| grp(c, 1).to_string());
    t = replace_all(re!(r"([,;:])(?=[A-Za-z])"), &t, |c| format!("{} ", grp(c, 1)));
    t = replace_all(re!(r"([a-z][.!?])(?=[A-Z])"), &t, |c| format!("{} ", grp(c, 1)));
    t = replace_lit(re!(r"\.{2,}(?!\.)"), &t, ".");
    t = replace_lit(re!(r",{2,}"), &t, ",");
    t = replace_all(re!(r"([,;:])\s*([.!?])"), &t, |c| grp(c, 2).to_string());
    t = replace_lit(re!(r"[ \t]*\n[ \t]*"), &t, "\n");
    t = replace_lit(re!(r"\n{3,}"), &t, "\n\n");
    t = t.trim().to_string();
    if english {
        t = replace_lit(re!(r"\bi\b(?![.\w])"), &t, "I");
        t = replace_all(re!(r"(?i)\bi'(m|ll|ve|d)\b"), &t, |c| format!("I'{}", grp(c, 1).to_lowercase()));
    }
    if capitalize {
        t = smart::capitalize_sentences(&t);
        if english {
            t = smart::proper_nouns(&t);
        }
    }
    t
}

#[derive(Clone, Debug, Default)]
pub struct Signals {
    pub words: usize,
    pub sentences: usize,
    pub list_cue: bool,
    pub email_cue: bool,
    pub structure_cue: bool,
    pub question_like: bool,
}

pub fn analyze(text: &str) -> Signals {
    let lower = text.to_lowercase();
    Signals {
        words: word_count(text),
        sentences: smart::split_sentences(text).len(),
        list_cue: is_match(
            re!(
                r"\b(bullet(ed)? list|numbered list|check ?list|bullet points?|(to[- ]?do|shopping|grocery|packing|reading|wish) list|list (these|this|the|those|them|out)|make (it |this |that )?(a |into a )?list|as a list|in a list|first(ly)?[, ]|second(ly)?[, ]|third(ly)?[, ]|next item|step one|step 1|number one|\bone\b.*\btwo\b.*\bthree\b)"
            ),
            &lower,
        ) || is_match(re!(r"(?m)^\s*[-*\d]+[.)]?\s"), text)
            || smart::has_enumeration(text),
        email_cue: is_match(
            re!(
                r"\b(dear|hi|hello|hey)\s+[a-z]+[,.]|\b(best regards|kind regards|sincerely|thanks,|cheers,|subject( line)?)\b"
            ),
            &lower,
        ),
        structure_cue: is_match(re!(r"\b(heading|subheading|section|paragraph)\b"), &lower),
        question_like: text.trim_end().ends_with('?'),
    }
}

pub struct LocalOpts {
    pub kind: String,
    pub remove_fillers: bool,
    pub spoken_commands: bool,
    pub auto_capitalize: bool,
    pub intensity: i64,
    pub language: String,
    pub terminal_punct: Option<bool>,
    pub email_format: bool,
    pub auto_lists: bool,
    pub grammar: bool,
    pub formalize: bool,
    pub expand_contractions: bool,
    pub humanize: String,
    pub excluded: Vec<String>,
    pub self_correct: bool,
}
impl Default for LocalOpts {
    fn default() -> Self {
        LocalOpts {
            kind: "auto".into(),
            remove_fillers: true,
            spoken_commands: true,
            auto_capitalize: true,
            intensity: 2,
            language: "auto".into(),
            terminal_punct: None,
            email_format: false,
            auto_lists: false,
            grammar: false,
            formalize: false,
            expand_contractions: false,
            humanize: "off".into(),
            excluded: vec![],
            self_correct: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LocalOut {
    pub text: String,
    pub signals: Signals,
    pub commands: bool,
    pub list_done: bool,
    pub had_punct: bool,
}

pub fn local_format(raw: &str, o: &LocalOpts, personal: &Value) -> LocalOut {
    let mut text = raw.trim().to_string();
    if text.is_empty() {
        return LocalOut { signals: analyze(""), ..Default::default() };
    }
    text = apply_personal(&text, personal);
    let (mut commands, mut list_done) = (false, false);
    if o.spoken_commands && o.kind != "code" {
        let (t, used) = spoken_emoji(&text);
        text = t;
        commands = used;
        let (t, used) = spoken_punctuation(&text);
        text = t;
        commands = commands || used;
    }
    if o.remove_fillers {
        text = remove_fillers(&text, o.intensity >= 2);
    }
    if o.intensity >= 1 {
        text = dedupe(&text);
    }
    if o.self_correct {
        let (t, used) = super::selfcorrect::apply(&text);
        text = t;
        commands = commands || used;
    }
    let english = (o.language == "auto" || o.language.to_lowercase().starts_with("en"))
        && text.chars().all(|c| (c as u32) < 128 || c == '…' || c == '—');
    text = tidy(&text, o.auto_capitalize && o.kind != "code", english);
    // lists are made everywhere except in code; requests to an AI and plain text fields get them too
    let structured = o.kind != "code";
    if o.kind == "email" && o.email_format {
        text = smart::email_format(&text, o.auto_lists);
    } else if o.auto_lists && structured {
        let a = smart::auto_list(&text);
        if a != text {
            list_done = true;
            text = a;
        }
    }
    if o.grammar && o.kind != "code" && english {
        text = smart::grammar_fix(
            &text,
            &smart::GrammarOpts {
                formalize: o.formalize,
                kind: &o.kind,
                terminal_punct: o.terminal_punct,
                expand: o.expand_contractions,
            },
        );
    } else if o.terminal_punct == Some(false) {
        text = replace_lit(re!(r"(?<=\w)\.$"), &text, "");
    }
    if o.kind != "code" {
        text = smart::humanize(&text, &o.humanize, o.formalize);
    }
    text = smart::exclude_chars(&text, &o.excluded);
    LocalOut { signals: analyze(&text), text, commands, list_done, had_punct: is_match(re!(r"[.!?]"), raw) }
}

#[cfg(test)]
mod emoji_tests {
    use super::*;
    fn e(s: &str) -> String {
        spoken_emoji(s).0
    }

    #[test]
    fn spoken_punctuation_becomes_the_character_without_doubling() {
        let f = |s: &str| spoken_punctuation(s).0;
        assert_eq!(f("what time is the meeting question mark"), "what time is the meeting?");
        assert_eq!(f("What time is the meeting? Question mark."), "What time is the meeting?");
        assert_eq!(f("wow that is great exclamation point"), "wow that is great!");
        assert_eq!(f("hi sam comma thanks"), "hi sam, thanks");
        assert_eq!(f("Hi Sam, comma, thanks"), "Hi Sam, thanks");
        assert_eq!(f("that is all period"), "that is all.");
        assert_eq!(f("for a period of time"), "for a period of time");
    }

    #[test]
    fn layout_and_key_words_are_only_words() {
        let on = LocalOpts { auto_lists: true, ..Default::default() };
        for said in ["see you soon press enter", "press tab", "thanks new line bye", "add a space"] {
            let out = local_format(said, &on, &serde_json::json!({})).text;
            assert_eq!(out.to_lowercase().trim_end_matches('.'), said, "{out}");
        }
    }

    #[test]
    fn spoken_emoji_become_characters() {
        assert_eq!(e("that works smiley face"), "that works 🙂");
        assert_eq!(e("thanks smiley emoji."), "thanks 🙂.");
        assert_eq!(e("Smiley Face"), "🙂");
        assert_eq!(e("send a thumbs up emoji please"), "send 👍 please");
        assert_eq!(e("love it heart emoji and fire emoji"), "love it ❤️ and 🔥");
        assert_eq!(e("so sad crying face emoji"), "so sad 😢");
        assert_eq!(e("ok winking face"), "ok 😉");
    }

    #[test]
    fn ordinary_words_are_left_alone() {
        for s in [
            "give it a thumbs up",
            "the fire is hot",
            "she has a happy face",
            "I like emoji",
            "light the star",
            "a sad face",
        ] {
            assert_eq!(e(s), s, "{s}");
        }
        assert!(!spoken_emoji("hello there").1);
        let in_code = LocalOpts { kind: "code".into(), ..Default::default() };
        assert!(!local_format("hi smiley face", &in_code, &serde_json::json!({})).text.contains('🙂'));
    }
}
