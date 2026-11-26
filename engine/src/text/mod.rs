pub mod join;
pub mod local;
pub mod selfcorrect;
pub mod smart;

pub use fancy_regex::{Captures, Regex};

#[macro_export]
macro_rules! re {
    ($p:expr) => {{
        static R: std::sync::LazyLock<fancy_regex::Regex> =
            std::sync::LazyLock::new(|| fancy_regex::Regex::new($p).expect("regex"));
        &*R
    }};
}

pub fn slice_bytes(s: &str, from: usize, to: usize) -> &str {
    let mut a = from.min(s.len());
    let mut b = to.min(s.len());
    while a > 0 && !s.is_char_boundary(a) {
        a -= 1;
    }
    while b < s.len() && !s.is_char_boundary(b) {
        b += 1;
    }
    &s[a..b.max(a)]
}

pub fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

pub fn cap_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn replace_all(re: &Regex, text: &str, mut f: impl FnMut(&Captures) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for caps in re.captures_iter(text).flatten() {
        let m = caps.get(0).unwrap();
        out.push_str(&text[last..m.start()]);
        out.push_str(&f(&caps));
        last = m.end();
    }
    out.push_str(&text[last..]);
    out
}

pub fn replace_lit(re: &Regex, text: &str, with: &str) -> String {
    replace_all(re, text, |_| with.to_string())
}

pub fn grp<'a>(c: &'a Captures, i: usize) -> &'a str {
    c.get(i).map(|m| m.as_str()).unwrap_or("")
}

use serde_json::{json, Value};

pub fn opts_from_json(v: &Value) -> local::LocalOpts {
    let b = |k: &str, d: bool| v[k].as_bool().unwrap_or(d);
    let s = |k: &str, d: &str| v[k].as_str().unwrap_or(d).to_string();
    local::LocalOpts {
        kind: s("kind", "auto"),
        remove_fillers: b("removeFillers", true),
        spoken_commands: b("spokenCommands", true),
        auto_capitalize: b("autoCapitalize", true),
        intensity: v["intensity"].as_i64().unwrap_or(2),
        language: s("language", "auto"),
        terminal_punct: v["terminalPunct"].as_bool(),
        email_format: b("emailFormat", false),
        auto_lists: b("autoLists", false),
        grammar: b("grammar", false),
        formalize: b("formalize", false),
        expand_contractions: b("expandContractions", false),
        humanize: s("humanize", "off"),
        self_correct: b("selfCorrect", false),
        excluded: v["excluded"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(|x| x.to_string())).collect())
            .unwrap_or_default(),
    }
}

pub fn signals_json(s: &local::Signals) -> Value {
    json!({ "words": s.words, "sentences": s.sentences, "listCue": s.list_cue, "emailCue": s.email_cue, "structureCue": s.structure_cue, "questionLike": s.question_like })
}

pub fn op_local(req: &Value) -> Value {
    let o = opts_from_json(&req["opts"]);
    let r = local::local_format(req["raw"].as_str().unwrap_or(""), &o, &req["opts"]["personal"]);
    json!({ "text": r.text, "signals": signals_json(&r.signals), "commands": r.commands, "listDone": r.list_done, "hadPunct": r.had_punct })
}

#[cfg(test)]
mod everyday_speech {
    use super::*;

    fn clean(said: &str) -> String {
        let o = opts_from_json(
            &json!({ "spokenCommands": true, "grammar": true, "autoLists": true, "selfCorrect": true, "autoCapitalize": true, "removeFillers": true, "intensity": 2 }),
        );
        local::local_format(said, &o, &json!({ "vocabulary": [] })).text
    }

    #[test]
    fn numbers_said_twice_are_kept() {
        assert_eq!(
            clean("my phone number is five five five one two three four"),
            "My phone number is five five five one two three four."
        );
        assert_eq!(clean("it is a two two four three"), "It is a two two four three.");
        assert_eq!(clean("i want to to go home"), "I want to go home.");
    }

    #[test]
    fn contractions_are_added_only_where_the_word_is_one() {
        assert_eq!(
            clean("its going to cost about five hundred dollars"),
            "It's going to cost about five hundred dollars."
        );
        assert_eq!(clean("lets meet at nine"), "Let's meet at nine.");
        assert_eq!(clean("ok sounds good ill see you there"), "Ok sounds good I'll see you there.");
        assert_eq!(clean("youve been great and theres more"), "You've been great and there's more.");
        assert!(clean("the dog wagged its tail").contains("its tail"));
        assert!(clean("he lets the dog out").contains("lets the dog"));
        assert!(clean("she was ill last week").contains("ill last week"));
    }

    #[test]
    fn days_months_and_greeting_names_get_capitals() {
        assert_eq!(
            clean("the meeting is on tuesday march third at ten thirty"),
            "The meeting is on Tuesday March third at ten thirty."
        );
        assert!(clean("hey sam can you send me the report by friday thanks")
            .starts_with("Hey Sam, can you send me the report by Friday"));
        assert!(
            clean("hi maria thanks for the update i will review it tonight best regards sam")
                .ends_with("best regards Sam."),
            "{}",
            clean("hi maria thanks for the update i will review it tonight best regards sam")
        );
        assert_eq!(clean("hey look at this"), "Hey look at this.");
        assert_eq!(clean("hello there everyone"), "Hello there everyone");
        assert_eq!(clean("i may go to the march"), "I may go to the march.");
        assert_eq!(clean("that is the best idea"), "That is the best idea.");
    }
}

#[cfg(test)]
mod probe_formatting {
    use super::*;

    /// cargo test probe_formatting -- --ignored --nocapture
    #[test]
    #[ignore]
    fn probe_formatting() {
        let o = opts_from_json(
            &json!({ "spokenCommands": true, "grammar": true, "autoLists": true, "selfCorrect": true, "autoCapitalize": true, "removeFillers": true, "intensity": 2 }),
        );
        for said in [
            "hey sam can you send me the report by friday thanks",
            "i think we should um meet at three pm tomorrow",
            "so basically the meeting is at nine am and we need to bring the laptops",
            "dear john new paragraph i hope you are well",
            "what time is the meeting question mark",
            "i need milk eggs bread and coffee",
            "first open the app second click settings third turn it on",
            "its going to cost about five hundred dollars",
            "lets meet at eight pm no wait nine pm",
            "thank you very much i really appreciate it",
            "can you call me back when you get this",
            "ok sounds good ill see you there",
            "im not sure what you mean can you explain",
            "i dont know if its gonna work but lets try",
            "email me at john at gmail dot com",
            "the price is twenty five percent off",
            "yeah no i totally agree with you on that one",
            "hi maria thanks for the update i will review it tonight and get back to you tomorrow morning best regards sam",
            "we need to ship the feature by monday because the client is waiting and i cant afford any more delays",
            "do you want to grab lunch",
            "i went to the store and i bought some apples and then i came home",
            "wow that is amazing exclamation mark",
            "the meeting is on tuesday march third at ten thirty",
            "my phone number is five five five one two three four",
            "where are you going tonight",
            "i have a question about the invoice",
            "this is a test of the dictation system",
            "she said that she would be late",
            "umm so like i was thinking maybe we could uh try a different approach",
            "hello",
            "yes",
            "okay thanks bye",
            "we should use python for this but then again javascript might be better",
            "how are you doing today my friend",
        ] {
            let r = local::local_format(said, &o, &json!({ "vocabulary": [] }));
            println!("IN : {said}\nOUT: {}\n", r.text.replace('\n', " ⏎ "));
        }
    }

    #[test]
    fn lists_are_made_consistent_whatever_the_model_wrote() {
        assert_eq!(
            smart::normalize_lists("I need:\n• milk\n* eggs\n- bread"),
            "I need:\n- milk\n- eggs\n- bread"
        );
        assert_eq!(
            smart::normalize_lists("Steps:\n1) Open it\n1. Press the button\n7. Done"),
            "Steps:\n1. Open it\n2. Press the button\n3. Done"
        );
        assert_eq!(
            smart::normalize_lists("Agenda\n- Budget\n\n- Hiring\nThen lunch."),
            "Agenda\n\n- Budget\n- Hiring\n\nThen lunch."
        );
        assert_eq!(smart::normalize_lists("2. That is what I said."), "2. That is what I said.");
        assert_eq!(smart::normalize_lists("It was 5 years ago."), "It was 5 years ago.");
        assert_eq!(
            smart::normalize_lists("*Important* note.\n\nSecond paragraph."),
            "*Important* note.\n\nSecond paragraph."
        );
        assert_eq!(
            smart::normalize_lists("1. Plan\n  - Draft\n  - Review\n2. Build"),
            "1. Plan\n  - Draft\n  - Review\n2. Build"
        );
        let once = smart::normalize_lists("For the trip:\n- A\n- B\n\nThen we go.");
        assert_eq!(smart::normalize_lists(&once), once);
    }

    #[test]
    fn spoken_list_words_are_noticed_so_the_model_is_told_to_make_a_list() {
        for said in [
            "my shopping list is milk eggs bread and coffee",
            "put these in a checklist",
            "give me bullet points for the meeting",
            "add this to my to do list call the bank",
            "first book the venue second send the invites",
        ] {
            assert!(local::analyze(said).list_cue, "{said}");
        }
        for said in [
            "the following day we left early",
            "i read the list yesterday and it was fine",
            "we went to the store and bought milk",
        ] {
            assert!(!local::analyze(said).list_cue, "{said}");
        }
    }
}
