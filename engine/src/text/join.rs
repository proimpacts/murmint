use super::*;
use crate::re;

const CONTINUES: &[&str] = &[
    "and", "but", "or", "so", "because", "which", "that", "then", "although", "though", "while", "whereas",
    "if", "when", "where", "who", "whom", "whose", "as", "since", "until", "unless", "plus", "also", "yet",
    "nor", "whether", "before", "after", "once", "than", "like",
];
const DANGLING: &[&str] = &[
    "the", "a", "an", "to", "of", "in", "on", "at", "for", "with", "from", "by", "into", "onto", "about",
    "and", "or", "but", "because", "if", "when", "my", "your", "our", "their", "his", "its", "will", "would",
    "should", "could", "i", "we", "they", "he", "she", "every", "very", "which", "than",
];

const INTRO: &[&str] = &[
    "hi",
    "hello",
    "hey",
    "dear",
    "okay",
    "ok",
    "so",
    "well",
    "yes",
    "no",
    "yeah",
    "however",
    "actually",
    "anyway",
    "basically",
    "honestly",
    "first",
    "firstly",
    "second",
    "secondly",
    "also",
    "finally",
    "meanwhile",
    "therefore",
    "instead",
    "besides",
    "otherwise",
    "still",
    "sure",
    "right",
    "now",
    "thanks",
    "look",
    "listen",
    "unfortunately",
    "fortunately",
    "obviously",
    "personally",
    "generally",
    "usually",
    "sometimes",
    "maybe",
    "perhaps",
    "today",
    "tomorrow",
    "yesterday",
    "tonight",
    "for",
    "in",
    "on",
    "by",
    "at",
];
const FRONTED: &[&str] = &[
    "when", "whenever", "if", "although", "though", "while", "since", "because", "after", "before", "once",
    "unless", "as", "whereas", "until",
];

/// Pausing while the keys are held is thinking, not ending a sentence: only a silence this long starts a new one,
/// so pieces of one thought are not chopped into fragments.
pub const SENTENCE_GAP_MS: u64 = 6000;
/// Unless it was already a whole sentence: this many words and a full stop, with a capital after it.
const FULL_SENTENCE_WORDS: usize = 10;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Join {
    Period,
    Comma,
    Nothing,
}

fn first_word(s: &str) -> String {
    s.trim_start_matches(|c: char| !c.is_alphanumeric())
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .next()
        .unwrap_or("")
        .to_lowercase()
}
fn last_word(s: &str) -> String {
    let t = s.trim_end_matches(|c: char| !c.is_alphanumeric());
    t.rsplit(|c: char| !(c.is_alphanumeric() || c == '\'')).next().unwrap_or("").to_lowercase()
}

pub fn join_kind(prev: &str, next: &str, gap_ms: Option<u64>) -> Join {
    let p = prev.trim();
    if p.ends_with('?') || p.ends_with('!') || p.ends_with("...") || p.ends_with('…') || p.ends_with('\n') {
        return Join::Period;
    }
    let (lw, fw) = (last_word(p), first_word(next));
    if DANGLING.contains(&lw.as_str()) {
        return Join::Nothing;
    }
    if CONTINUES.contains(&fw.as_str()) {
        return Join::Comma;
    }
    if let Some(body) = p.strip_suffix('.') {
        let sentence = body.rsplit(['.', '!', '?', '\n']).next().unwrap_or(body);
        if sentence.split_whitespace().count() >= FULL_SENTENCE_WORDS
            && next.trim().starts_with(|c: char| c.is_uppercase())
        {
            return Join::Period;
        }
    }
    let pw = p.split_whitespace().count();
    let pf = first_word(p);
    if pw <= 4 && INTRO.contains(&pf.as_str()) {
        return Join::Comma;
    }
    let new_clause = starts_clause(next) && pw >= 3;
    if next.trim().chars().next().is_some_and(|c| c.is_lowercase()) {
        return if new_clause { Join::Comma } else { Join::Nothing };
    }
    match gap_ms {
        Some(g) if g >= SENTENCE_GAP_MS => Join::Period,
        Some(_) => {
            if new_clause {
                Join::Comma
            } else {
                Join::Nothing
            }
        }
        None => {
            if FRONTED.contains(&pf.as_str()) && pw <= 10 {
                Join::Comma
            } else {
                Join::Period
            }
        }
    }
}

const SUBJECTS: &[&str] = &[
    "i", "i'm", "i've", "i'll", "i'd", "we", "you", "he", "she", "they", "it", "it's", "there", "this",
    "that", "these", "those", "everyone", "nobody", "someone",
];
const VERBS: &[&str] = &[
    "is", "are", "was", "were", "am", "be", "been", "will", "would", "can", "could", "should", "shall",
    "may", "might", "must", "do", "does", "did", "have", "has", "had", "need", "needs", "needed", "want",
    "wants", "wanted", "think", "thinks", "thought", "know", "knows", "knew", "say", "says", "said", "go",
    "goes", "went", "get", "gets", "got", "make", "makes", "made", "see", "sees", "saw", "come", "comes",
    "came", "look", "looks", "feel", "feels", "felt", "seem", "seems", "seemed", "bought", "take", "takes",
    "took", "tell", "told", "like", "likes", "love", "loves", "called", "sent", "wrote", "asked",
];

fn starts_clause(piece: &str) -> bool {
    let words: Vec<String> = piece
        .split_whitespace()
        .take(3)
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .collect();
    match words.as_slice() {
        [] => false,
        [w, ..] if SUBJECTS.contains(&w.as_str()) => true,
        [_, v, ..] if VERBS.contains(&v.as_str()) => true,
        [_, _, v] if VERBS.contains(&v.as_str()) => true,
        _ => false,
    }
}

pub fn run_on(piece: &str) -> String {
    let fw = first_word(piece);
    if fw == "i" || fw.starts_with("i'") {
        piece.to_string()
    } else {
        lower_first(piece)
    }
}

const LOWER_AFTER_COMMA: &[&str] = &[
    "we", "you", "they", "he", "she", "it", "there", "this", "these", "those", "the", "a", "an", "my", "our",
    "your", "their", "please", "let", "let's", "can", "could", "would", "should", "will", "do", "does",
    "did", "is", "are", "was", "were", "thanks", "thank", "maybe", "just", "not", "no", "yes", "then", "now",
    "here", "what", "how", "why", "all", "some", "one",
];

// Words that carry on a sentence already in the text box: "Hey this." then "and then…".
const JOINERS: &[&str] = &["and", "but", "or", "so", "because", "then", "yet", "plus"];

/// Box joining: when the text before the cursor ends a sentence with a full stop and the new text starts with a
/// joining word, how many characters to erase (the full stop and the spaces after it) and what to type instead,
/// so "Hey this." + "And that." reads "Hey this, and that.". A new line, "..." or no joining word leave it alone.
pub fn continue_in_box(before: &str, text: &str) -> Option<(usize, String)> {
    let body = before.trim_end_matches([' ', '\t', '\u{a0}']);
    let spaces = before[body.len()..].chars().count();
    let stem = body.strip_suffix('.')?;
    if !stem.chars().last()?.is_alphanumeric() {
        return None;
    }
    let next = text.trim_start();
    if !JOINERS.contains(&first_word(next).as_str()) {
        return None;
    }
    Some((spaces + 1, format!(", {}", lower_first(next))))
}

pub fn after_comma(piece: &str) -> String {
    let fw = first_word(piece);
    if CONTINUES.contains(&fw.as_str()) || LOWER_AFTER_COMMA.contains(&fw.as_str()) {
        lower_first(piece)
    } else {
        piece.to_string()
    }
}

fn strip_closing_period(s: &str) -> String {
    re!(r"\.$").replace(s.trim_end(), "").to_string()
}

pub fn join_pieces(pieces: &[(String, Option<u64>)]) -> String {
    let mut out = String::new();
    for (raw, gap) in pieces {
        let piece = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if piece.is_empty() {
            continue;
        }
        if out.is_empty() {
            out = piece;
            continue;
        }
        match join_kind(&out, &piece, *gap) {
            Join::Period => {
                out.push(' ');
                out.push_str(&piece);
            }
            Join::Comma => {
                let base = strip_closing_period(&out);
                let base = base.trim_end_matches(',').to_string();
                out = format!("{base}, {}", after_comma(&piece));
            }
            Join::Nothing => {
                let base = strip_closing_period(&out);
                out = format!("{base} {}", run_on(&piece));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_sentences_keep_their_full_stop_across_a_short_pause() {
        let join = |a: &str, b: &str| join_pieces(&[(a.into(), None), (b.into(), Some(1500))]);
        let long = "The budget review went well and we are slightly over on travel.";
        assert_eq!(join(long, "The designer starts on Monday."), format!("{long} The designer starts on Monday."));
        assert_eq!(join("I went to the store.", "I bought some milk."), "I went to the store, I bought some milk.");
    }

    #[test]
    fn a_sentence_in_the_box_carries_on_with_a_comma_and_lowercase() {
        assert_eq!(continue_in_box("Hey this.", "And then we left."), Some((1, ", and then we left.".into())));
        assert_eq!(continue_in_box("Hey this. ", "and that"), Some((2, ", and that".into())));
        assert_eq!(continue_in_box("It works.", "But not on Mac."), Some((1, ", but not on Mac.".into())));
        // only when the new words carry the sentence on
        assert_eq!(continue_in_box("Hey this.", "The build is green."), None);
        // a new line, an ellipsis or no full stop is left as it is
        assert_eq!(continue_in_box("Hey this.\n", "And that."), None);
        assert_eq!(continue_in_box("Hmm...", "And that."), None);
        assert_eq!(continue_in_box("Hey this", "and that"), None);
        assert_eq!(continue_in_box("", "And that."), None);
    }
    fn j(p: &[&str]) -> String {
        join_pieces(&p.iter().map(|s| (s.to_string(), None)).collect::<Vec<_>>())
    }
    fn jg(p: &[(&str, u64)]) -> String {
        join_pieces(&p.iter().map(|(s, g)| (s.to_string(), Some(*g))).collect::<Vec<_>>())
    }

    #[test]
    fn a_pause_in_the_middle_of_a_sentence_is_a_comma() {
        assert_eq!(
            j(&["We went to the store.", "And bought milk."]),
            "We went to the store, and bought milk."
        );
        assert_eq!(
            j(&["I wanted to call you.", "But you were busy."]),
            "I wanted to call you, but you were busy."
        );
        assert_eq!(
            j(&["It was late.", "Because the train was delayed."]),
            "It was late, because the train was delayed."
        );
        assert_eq!(j(&["Send it to Sam.", "or Maria."]), "Send it to Sam, or Maria.");
    }

    #[test]
    fn a_sentence_that_cannot_end_there_just_continues() {
        assert_eq!(j(&["We went to the.", "Store."]), "We went to the store.");
        assert_eq!(j(&["We should send it to our.", "Team lead."]), "We should send it to our team lead.");
    }

    #[test]
    fn real_sentence_ends_stay_full_stops() {
        assert_eq!(
            j(&["We went to the store.", "Then we went home."]),
            "We went to the store, then we went home.",
            "a continuing word makes it a comma"
        );
        assert_eq!(
            j(&["The build is green.", "Please review the PR."]),
            "The build is green. Please review the PR."
        );
        assert_eq!(j(&["Are you free?", "I can meet at three."]), "Are you free? I can meet at three.");
        assert_eq!(j(&["That was great!", "And we won."]), "That was great! And we won.");
    }

    #[test]
    fn whitespace_is_tidied_and_empty_pieces_skipped() {
        assert_eq!(
            j(&["  we went  to the store. ", "", "and  bought milk"]),
            "we went to the store, and bought milk"
        );
        assert_eq!(j(&[]), "");
        assert_eq!(j(&["only one."]), "only one.");
    }

    #[test]
    fn the_join_decision() {
        assert_eq!(join_kind("We went to the store.", "And bought milk.", None), Join::Comma);
        assert_eq!(join_kind("We went to the", "store", None), Join::Nothing);
        assert_eq!(join_kind("We went home.", "It was late.", None), Join::Period);
    }

    #[test]
    fn a_short_pause_is_a_comma_and_a_long_one_is_a_full_stop() {
        assert_eq!(
            jg(&[("I went to the store.", 0), ("I bought some milk.", 900)]),
            "I went to the store, I bought some milk."
        );
        assert_eq!(
            jg(&[("I met Maria.", 0), ("Maria was kind.", 700)]),
            "I met Maria, Maria was kind.",
            "names keep their capital"
        );
        assert_eq!(
            jg(&[("I went to the store.", 0), ("I bought some milk.", 7000)]),
            "I went to the store. I bought some milk."
        );
        assert_eq!(
            jg(&[("Are you free?", 0), ("I can meet at three.", 600)]),
            "Are you free? I can meet at three.",
            "a question mark is the speaker's own"
        );
        assert_eq!(jg(&[("Hi Sam.", 0), ("Thanks for the update.", 3000)]), "Hi Sam, thanks for the update.");
    }

    #[test]
    fn a_pause_while_the_keys_are_held_does_not_split_the_sentence() {
        assert_eq!(
            jg(&[("I went to the store.", 0), ("I bought some milk.", 2400)]),
            "I went to the store, I bought some milk.",
            "a few seconds of silence is thinking time"
        );
        assert_eq!(
            jg(&[("The launch is on Friday.", 0), ("The venue is booked.", 3500)]),
            "The launch is on Friday. The venue is booked.".replace(". The", ", the")
        );
    }

    #[test]
    fn a_pause_inside_a_phrase_is_not_a_comma() {
        assert_eq!(
            jg(&[("I think the best way to do this.", 0), ("Is to start early.", 700)]),
            "I think the best way to do this is to start early."
        );
        assert_eq!(
            jg(&[("Can you send the report.", 0), ("To the whole team by Friday.", 800)]),
            "Can you send the report to the whole team by Friday."
        );
        assert_eq!(
            jg(&[("I talked to the team.", 0), ("I think we are ready.", 800)]),
            "I talked to the team, I think we are ready.",
            "a new clause still gets its comma"
        );
        assert_eq!(
            jg(&[("We tested it twice.", 0), ("I mean three times.", 600)]),
            "We tested it twice, I mean three times."
        );
    }

    #[test]
    fn an_opening_clause_waits_for_its_main_clause() {
        assert_eq!(j(&["When I was young.", "I lived in Paris."]), "When I was young, I lived in Paris.");
        assert_eq!(j(&["However.", "We should go."]), "However, we should go.");
    }
}
