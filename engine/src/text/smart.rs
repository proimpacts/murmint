use super::*;
use crate::re;
use std::sync::LazyLock;

pub fn match_case(orig: &str, repl: &str) -> String {
    if orig.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        cap_first(repl)
    } else {
        repl.to_string()
    }
}
fn cap_words(s: &str) -> String {
    s.split_whitespace().map(cap_first).collect::<Vec<_>>().join(" ")
}
fn is_match(re: &Regex, s: &str) -> bool {
    re.is_match(s).unwrap_or(false)
}

pub fn capitalize_sentences(t: &str) -> String {
    let re = re!(r#"(^(?:[-*]\s|\d+\.\s|#+\s)?|[.!?]\s+|\n(?:[-*]\s|\d+\.\s|#+\s)?|"\s?)([a-z])"#);
    let abbrev = re!(r"(?i)(?:e\.g|i\.e|etc|vs|mr|mrs|ms|dr|prof|st|no|fig|approx|inc|ltd)\.\s*$");
    let sent_end = re!(r"[.!?]\s+$");
    replace_all(re, t, |c| {
        let m = c.get(0).unwrap();
        let (pre, ch) = (grp(c, 1), grp(c, 2));
        if pre.starts_with('"') {
            let before = t[..m.start()].trim_end();
            let opens_quote = t[..m.start()].matches('"').count() % 2 == 0;
            let starts_sentence = before.is_empty() || before.ends_with([',', ':', '.', '!', '?', '\n']);
            if !(opens_quote && starts_sentence) {
                return m.as_str().to_string();
            }
        }
        if is_match(sent_end, pre) {
            let from = m.start().saturating_sub(12);
            if is_match(abbrev, slice_bytes(t, from, m.start() + pre.len())) {
                return m.as_str().to_string();
            }
        }
        format!("{pre}{}", ch.to_uppercase())
    })
}

pub fn split_sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let abbrev = re!(r"(?i)(?:^|\s)(?:e\.g|i\.e|etc|vs|mr|mrs|ms|dr|prof|st|no|fig|approx|inc|ltd|[a-z])\.$");
    let (mut out, mut cur, mut i) = (Vec::<String>::new(), String::new(), 0usize);
    while i < chars.len() {
        let c = chars[i];
        cur.push(c);
        if c == '\n' {
            if !cur.trim().is_empty() {
                out.push(cur.trim().to_string());
            }
            cur.clear();
            i += 1;
            continue;
        }
        if ".!?".contains(c) {
            let mut j = i + 1;
            while j < chars.len() && "\"')".contains(chars[j]) {
                cur.push(chars[j]);
                j += 1;
            }
            if j < chars.len() && chars[j].is_whitespace() {
                let mut k = j;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                let next_ok = k < chars.len()
                    && (chars[k].is_ascii_uppercase() || chars[k].is_ascii_digit() || chars[k] == '"');
                let is_abbrev = is_match(abbrev, cur.trim_end());
                if next_ok && (c != '.' || !is_abbrev) {
                    out.push(cur.trim().to_string());
                    cur.clear();
                    i = k;
                    continue;
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn strip_end(s: &str) -> String {
    re!(r"[.!?]+$").replace(s.trim(), "").to_string()
}

pub fn auto_list(text: &str) -> String {
    if is_match(re!(r"(?:^|\n)- "), text) || is_match(re!(r"(?m)^\d+\.\s"), text) {
        return text.to_string();
    }
    let mut text = text.to_string();
    let ordinals =
        re!(r"(?i)\b(?:first(?:ly)?|second(?:ly)?|third(?:ly)?|fourth|fifth)\b").find_iter(&text).count();
    if ordinals >= 3 {
        text = replace_all(
            re!(r"(?i)(?<=\w),\s+(first(?:ly)?|second(?:ly)?|third(?:ly)?|fourth|fifth|finally|lastly)\b"),
            &text,
            |c| format!(". {}", cap_first(grp(c, 1))),
        );
    }
    let ord = re!(
        r"(?i)^(?:(first(?:ly)?)|second(?:ly)?|third(?:ly)?|fourth(?:ly)?|fifth|sixth|next|then|finally|lastly|last)\b[,:]?\s*(.+)$"
    );
    let sents = split_sentences(&text);
    let start = sents.iter().position(|s| ord.captures(s).ok().flatten().is_some_and(|c| c.get(1).is_some()));
    if let Some(start) = start {
        let mut j = start;
        while j < sents.len() && is_match(ord, &sents[j]) {
            j += 1;
        }
        if j - start >= 3 {
            let mut intro = sents[..start].join(" ");
            if intro.ends_with('.')
                && is_match(
                    re!(
                        r"(?i)\b(following|steps?|things?|points?|items?|reasons?|plan|agenda|priorities|tasks?|process)\b"
                    ),
                    &intro,
                )
            {
                intro.pop();
                intro.push(':');
            }
            let items: Vec<String> = sents[start..j]
                .iter()
                .enumerate()
                .map(|(n, s)| {
                    let body =
                        ord.captures(s).ok().flatten().map(|c| grp(&c, 2).to_string()).unwrap_or_default();
                    format!("{}. {}", n + 1, cap_first(&strip_end(&body)))
                })
                .collect();
            let mut out = if intro.is_empty() { String::new() } else { format!("{intro}\n") };
            out.push_str(&items.join("\n"));
            if j < sents.len() {
                out.push_str("\n\n");
                out.push_str(&sents[j..].join(" "));
            }
            return out;
        }
    }
    let following = re!(
        r"(?is)^(.*?\b(?:the following|following)(?:\s+(?:items|things|points|steps|topics|tasks|people))?)\s*[:,]?\s+((?:[^,.;:!?\n]{1,48},\s+){2,}(?:and\s+|or\s+)?[^,.;:!?\n]{1,48})([.!?]|$)(.*)$"
    );
    if let Ok(Some(m)) = following.captures(&text) {
        let items: Vec<String> = grp(&m, 2)
            .split(',')
            .map(|p| re!(r"(?i)^\s*(?:and|or)\s+").replace(p, "").trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if items.len() >= 3 {
            let mut out = format!(
                "{}:\n{}",
                grp(&m, 1).trim(),
                items.iter().map(|i| format!("- {}", cap_first(i))).collect::<Vec<_>>().join("\n")
            );
            let rest = grp(&m, 4).trim();
            if !rest.is_empty() {
                out.push_str("\n\n");
                out.push_str(rest);
            }
            return out;
        }
    }
    enumerate(&text).unwrap_or(text)
}

const LEAD_INS: &str = r"(?i)\b(?:need|needs|needed|bring|buy|get|grab|pick up|include|includes|including|such as|want|wants|have|has|got|order|pack|add|remember|are|is|was|were|like|things|items|stuff)\b:?\s+|:\s*";
const MAX_ITEM_WORDS: usize = 4;

fn words_in(s: &str) -> usize {
    s.split_whitespace().count()
}

// "I need an apple" -> "an apple"
fn after_lead_in(p: &str) -> &str {
    match Regex::new(LEAD_INS).ok().and_then(|re| re.find_iter(p).flatten().last()) {
        Some(m) if m.end() < p.len() => p[m.end()..].trim(),
        _ => p,
    }
}

fn series(sentence: &str) -> Option<(String, Vec<String>)> {
    let s = sentence.trim();
    if s.ends_with('?')
        || is_match(
            re!(r"(?i)^(?:hi|hello|hey|dear|thanks|thank you|good (?:morning|afternoon|evening))\b"),
            s,
        )
    {
        return None;
    }
    let body = strip_end(s);
    let parts: Vec<&str> = body.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let mut items: Vec<String> = parts[1..parts.len() - 1].iter().map(|p| p.to_string()).collect();
    let last = parts[parts.len() - 1];
    let last_items: Vec<String> = re!(r"(?i)^(?:and|or)\s+")
        .replace(last, "")
        .split(" and ")
        .flat_map(|x| x.split(" or "))
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect();
    let joined_last = !is_match(re!(r"(?i)^(?:and|or)\s"), last) && last_items.len() == 2;
    // bare comma lists; the word check keeps "well, I think, maybe" out
    // only the first piece may carry a lead-in ("I need milk"); in a later piece "like" or "so" means it's talk, not an item
    let bare = parts.len() >= 3
        && parts.iter().enumerate().all(|(i, p)| {
            let item = if i == 0 { after_lead_in(p) } else { p };
            words_in(item) <= 3
                && !is_match(
                    re!(r"(?i)\b(?:i|you|we|they|he|she|it|is|was|are|were|be|been|went|go|think|so|well|maybe|but|because|if|then|that|this|just|really|yeah|yes|no|okay|ok|um|uh|oh|ah|hmm|wow|like|something|anything|everything|nothing|else|whatever|kind|sort|after|before|when|while|yesterday|today|tomorrow)\b"),
                    &re!(r"(?i)^(?:a|an|the|some)\s+").replace(item, ""),
                )
        });
    if !is_match(re!(r"(?i)^(?:and|or)\s"), last) && !joined_last && !bare {
        return None;
    }
    items.extend(last_items);
    let first = parts[0];
    let (lead, first_item) =
        match Regex::new(LEAD_INS).ok().and_then(|re| re.find_iter(first).flatten().last()) {
            Some(m) if m.end() < first.len() => (
                first[..m.end()].trim().trim_end_matches(':').trim().to_string(),
                first[m.end()..].trim().to_string(),
            ),
            _ => (String::new(), first.to_string()),
        };
    items.insert(0, first_item);
    // "milk, eggs and cheese for the trip": the trailing phrase belongs to the lead-in, not to the last item
    let mut lead = lead;
    if let Some(last) = items.last_mut() {
        if let Ok(Some(m)) = re!(r"(?i)^(.+?)\s+((?:for|at|on|in|from|by|before|after|this|next|tomorrow|tonight|today)\b.*)$").captures(last) {
            let (item, tail) = (grp(&m, 1).to_string(), grp(&m, 2).to_string());
            if !lead.is_empty() && words_in(&item) <= 2 && words_in(&tail) <= 4 {
                let keep_cap = is_match(re!(r"^I\b"), &lead);
                lead = format!("{}, {}", cap_first(&tail), if keep_cap { lead.clone() } else { lower_first(&lead) });
                *last = item;
            }
        }
    }
    if items.len() < 3 || items.iter().any(|i| i.is_empty() || words_in(i) > MAX_ITEM_WORDS) {
        return None;
    }
    Some((lead, items))
}

pub fn enumerate(text: &str) -> Option<String> {
    if text.contains('\n') && is_match(re!(r"(?m)^\s*(?:[-*]|\d+\.)\s"), text) {
        return None;
    }
    let sents = split_sentences(text);
    let (idx, (lead, items)) = sents.iter().enumerate().find_map(|(i, s)| series(s).map(|x| (i, x)))?;
    let list = items.iter().map(|i| format!("- {}", cap_first(i))).collect::<Vec<_>>().join("\n");
    let mut out = String::new();
    let before = sents[..idx].join(" ");
    if !before.is_empty() {
        out.push_str(&before);
        out.push_str("\n\n");
    }
    if !lead.is_empty() {
        out.push_str(&cap_first(&lead));
        out.push_str(":\n");
    }
    out.push_str(&list);
    let after = sents[idx + 1..].join(" ");
    if !after.is_empty() {
        out.push_str("\n\n");
        out.push_str(&after);
    }
    Some(out)
}

pub fn has_enumeration(text: &str) -> bool {
    split_sentences(text).iter().any(|s| series(s).is_some())
}

fn paragraphs(body: &str) -> String {
    if body.contains('\n') || word_count(body) < 60 {
        return body.to_string();
    }
    let (mut paras, mut cur, mut n, mut w) = (Vec::<String>::new(), Vec::<String>::new(), 0, 0);
    for s in split_sentences(body) {
        w += word_count(&s);
        n += 1;
        cur.push(s);
        if n >= 3 || w >= 55 {
            paras.push(cur.join(" "));
            cur.clear();
            n = 0;
            w = 0;
        }
    }
    if !cur.is_empty() {
        paras.push(cur.join(" "));
    }
    paras.join("\n\n")
}

/// Breaks any long block of prose into paragraphs. Lists and blocks that already have line breaks are left alone.
pub fn auto_paragraphs(text: &str) -> String {
    let list_item = re!(r"^\s*(?:[-*]|\d+[.)])\s");
    text.split("\n\n")
        .map(|block| if is_match(list_item, block) { block.to_string() } else { paragraphs(block) })
        .collect::<Vec<_>>()
        .join("\n\n")
}

const LOWER_GREET: &[&str] =
    &["team", "everyone", "all", "there", "folks", "guys", "friends", "everybody", "both", "y'all"];
const NOT_NAMES: &[&str] = &[
    "for", "again", "so", "much", "a", "the", "everyone", "all", "you", "that", "this", "it", "and", "in",
    "very", "guys", "team", "to", "my", "your", "i", "we", "if", "but", "on", "at", "with", "of", "is",
];

pub fn email_format(text: &str, auto_lists: bool) -> String {
    let mut body = text.trim().to_string();
    let (mut greeting, mut signoff) = (String::new(), String::new());

    let g1 = re!(
        r"(?i)^\s*(hi|hello|hey|dear|good (?:morning|afternoon|evening))\s+((?:[\w'’-]+)(?:\s+[\w'’-]+)?)\s*,\s*"
    );
    let g2 = re!(r"(?i)^\s*(hi|hello|hey|dear)\s+([A-Za-z][\w'’-]*)\s*[.,!:]?\s+(?=\S)");
    let g3 = re!(
        r"(?i)^\s*(hi there|hello there|hey there|hi|hello|hey|good (?:morning|afternoon|evening))\s*[,.!]\s*"
    );
    if let Ok(Some(m)) = g1.captures(&body) {
        let nm = grp(&m, 2).to_lowercase();
        greeting = format!(
            "{} {},",
            cap_first(&grp(&m, 1).to_lowercase()),
            if LOWER_GREET.contains(&nm.as_str()) { nm.clone() } else { cap_words(&nm) }
        );
        let len = m.get(0).unwrap().end();
        body = body[len..].to_string();
    } else if let Some(m) = g2
        .captures(&body)
        .ok()
        .flatten()
        .filter(|m| grp(m, 2).chars().next().is_some_and(|c| c.is_ascii_uppercase()) && grp(m, 2) != "I")
    {
        greeting = format!("{} {},", cap_first(&grp(&m, 1).to_lowercase()), grp(&m, 2));
        let len = m.get(0).unwrap().end();
        body = body[len..].to_string();
    } else if let Ok(Some(m)) = g3.captures(&body) {
        greeting = format!("{},", cap_first(&grp(&m, 1).to_lowercase()));
        let len = m.get(0).unwrap().end();
        body = body[len..].to_string();
    }

    let so = re!(
        r"(?i)(?:^|(?<=[.!?])\s+)(best regards|kind regards|warm regards|many thanks|thank you|thanks|regards|best wishes|best|sincerely|cheers|talk soon|take care|yours truly)\b(\s*[,.!]?)\s*([^.!?\n]*?)\s*[.!]?\s*$"
    );
    if let Ok(Some(m)) = so.captures(&body) {
        let start = m.get(0).unwrap().start();
        if start > 0 {
            let phrase = cap_first(&grp(&m, 1).to_lowercase());
            let tail = grp(&m, 3).trim().to_string();
            let words: Vec<&str> = tail.split_whitespace().collect();
            let had_comma = grp(&m, 2).contains(',');
            let name_ok = words.is_empty()
                || (words.len() <= 3
                    && words.iter().all(|w| w.chars().next().is_some_and(|c| c.is_uppercase())))
                || (had_comma
                    && words.len() == 1
                    && !NOT_NAMES.contains(&words[0].to_lowercase().as_str())
                    && is_match(re!(r"^[\p{L}'’-]+$"), words[0]));
            if name_ok {
                signoff = if !words.is_empty() {
                    format!("{phrase},\n{}", cap_words(&tail))
                } else if ["thanks", "thank you", "many thanks"].contains(&phrase.to_lowercase().as_str()) {
                    format!("{phrase}.")
                } else {
                    format!("{phrase},")
                };
                body = body[..start].trim().to_string();
            }
        }
    }
    body = capitalize_sentences(body.trim());
    if auto_lists {
        body = auto_list(&body);
    }
    if !body.contains('\n') {
        body = paragraphs(&body);
    }
    [greeting, body.trim().to_string(), signoff]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn contraction(w: &str) -> Option<&'static str> {
    Some(match w {
        "dont" => "don't",
        "cant" => "can't",
        "wont" => "won't",
        "doesnt" => "doesn't",
        "didnt" => "didn't",
        "isnt" => "isn't",
        "arent" => "aren't",
        "wasnt" => "wasn't",
        "werent" => "weren't",
        "hasnt" => "hasn't",
        "havent" => "haven't",
        "hadnt" => "hadn't",
        "shouldnt" => "shouldn't",
        "wouldnt" => "wouldn't",
        "couldnt" => "couldn't",
        "im" => "I'm",
        "ive" => "I've",
        "youre" => "you're",
        "theyre" => "they're",
        "thats" => "that's",
        "whats" => "what's",
        "youve" => "you've",
        "weve" => "we've",
        "theyve" => "they've",
        "youll" => "you'll",
        "theyll" => "they'll",
        "itll" => "it'll",
        "thatll" => "that'll",
        "theres" => "there's",
        "heres" => "here's",
        "wheres" => "where's",
        "whos" => "who's",
        "hes" => "he's",
        "shes" => "she's",
        "couldve" => "could've",
        "wouldve" => "would've",
        "shouldve" => "should've",
        "mustve" => "must've",
        _ => return None,
    })
}

pub fn an_needed(word: &str) -> Option<bool> {
    let w = word.to_lowercase();
    if word.chars().count() > 1 && word == word.to_uppercase() && word.chars().any(|c| c.is_ascii_uppercase())
    {
        return None;
    }
    if is_match(re!(r"^(hour|honest|honor|honour|heir|herb)"), &w) {
        return Some(true);
    }
    let first = w.chars().next()?;
    if "aeiou".contains(first) {
        let chars: Vec<char> = w.chars().collect();
        if w.starts_with("uni") && chars.get(3).is_some_and(|c| !"mnd".contains(*c)) {
            return Some(false);
        }
        if is_match(re!(r"^(use|usu|uti|eur|one|once|ubi|ew)"), &w) {
            return Some(false);
        }
        return Some(true);
    }
    Some(false)
}

pub fn polish_punctuation(text: &str) -> String {
    text.split('\n').map(polish_line).collect::<Vec<_>>().join("\n")
}

fn polish_line(line: &str) -> String {
    if line.trim().is_empty() {
        return line.to_string();
    }
    let mut t = line.to_string();
    t = replace_all(re!(r"(?i)\band\s+(?:slash\s+)?or\b"), &t, |c| {
        let m = grp(c, 0);
        if m.starts_with('A') {
            "And/or".into()
        } else {
            "and/or".to_string()
        }
    });
    t = replace_all(re!(r"\s*,\s*(and/or)\b(?:\s*,)?"), &t, |c| format!(" {}", grp(c, 1)));
    t = replace_all(re!(r"(?i)\b(either|neither|both)\s*,\s+"), &t, |c| format!("{} ", grp(c, 1)));
    t = replace_all(re!(r"(?i)\b((?:either|both)\s+[^,.;:!?\n]{1,40}?),\s+(or|and)\b"), &t, |c| {
        format!("{} {}", grp(c, 1), grp(c, 2))
    });
    t = replace_all(
        re!(
            r"(?i)\b(the|an|to|of|for|in|on|with|at|by|from|my|your|our|their|his|her|its|than|because)\s*,\s+(?=[a-z])"
        ),
        &t,
        |c| format!("{} ", grp(c, 1)),
    );
    t = replace_all(re!(r",\s*,+"), &t, |_| ",".to_string());
    t = replace_all(re!(r"(?<=\w)[ \t]+([,;:!?.])(?=\s|$)"), &t, |c| grp(c, 1).to_string());
    t = replace_all(re!(r"[,;]\s*([.!?])"), &t, |c| grp(c, 1).to_string());
    t = replace_all(
        re!(
            r"([^.!?,;:\n]*[^\s.!?,;:])\s+(and|but|so|yet|or)\s+(I|[Ww]e|[Yy]ou|[Hh]e|[Ss]he|[Tt]hey|[Ii]t|[Tt]here)\s+(?=[a-z'’]{2,})"
        ),
        &t,
        |c| {
            let (left, conj, subj) = (grp(c, 1), grp(c, 2), grp(c, 3));
            let lw = word_count(left);
            let skip = lw < 4
                || is_match(
                    re!(r"(?i)\b(either|neither|both|whether|between|not only|if|when|because|that)\b"),
                    left,
                )
                || !is_match(
                    re!(
                        r"(?i)\b(I|we|you|he|she|they|it|there|[a-z]+ (?:is|are|was|were|will|can|should|would|could|has|have|had|do|does|did)|[a-z]{3,}(?:ed|s))\b"
                    ),
                    left,
                );
            if skip {
                grp(c, 0).to_string()
            } else {
                format!("{left}, {conj} {subj} ")
            }
        },
    );
    t
}

pub struct GrammarOpts<'a> {
    pub formalize: bool,
    pub kind: &'a str,
    pub terminal_punct: Option<bool>,
    pub expand: bool,
}

pub fn grammar_fix(text: &str, o: &GrammarOpts) -> String {
    let mut t = replace_all(
        re!(
            r"(?i)\b(dont|cant|wont|doesnt|didnt|isnt|arent|wasnt|werent|hasnt|havent|hadnt|shouldnt|wouldnt|couldnt|im|ive|youre|theyre|thats|whats|youve|weve|theyve|youll|theyll|itll|thatll|theres|heres|wheres|whos|hes|shes|couldve|wouldve|shouldve|mustve)\b(?!')"
        ),
        text,
        |c| {
            let m = grp(c, 0);
            let r = contraction(&m.to_lowercase()).unwrap_or(m);
            if r.starts_with('I') {
                r.to_string()
            } else {
                match_case(m, r)
            }
        },
    );
    // its/lets/ill only expand before a word that implies the contraction
    t = replace_all(
        re!(
            r"(?i)\b(its)(\s+)(going|gonna|been|a|an|the|not|just|so|very|really|too|time|ok|okay|fine|good|bad|great|nice|getting|still|also|now|because|important|hard|easy|true|possible|like|about|all|only|already|supposed|always|never|late|early|worth|done|clear|probably|definitely|actually|pretty|quite|more|less|what|how|when|where|why|who|raining|snowing|cold|hot|today|tonight|tomorrow|my|your|our|their)\b"
        ),
        &t,
        |c| format!("{}{}{}", match_case(grp(c, 1), "it's"), grp(c, 2), grp(c, 3)),
    );
    t = replace_all(
        re!(
            r"(?i)\b(lets)(\s+)(go|meet|see|try|do|start|get|make|talk|take|check|look|move|keep|say|be|find|use|work|begin|finish|plan|call|set|think|hope|review|discuss|schedule|just|not|all|also|wait|stop|leave|focus|figure|pick|grab|continue|add|build|ship|test|fix|run|hold|stay|sit|walk|eat|order|book|reach|sync|catch|circle|touch|chat|hop|jump|dive)\b"
        ),
        &t,
        |c| format!("{}{}{}", match_case(grp(c, 1), "let's"), grp(c, 2), grp(c, 3)),
    );
    t = replace_all(
        re!(
            r"(?i)\b(ill)(\s+)(see|be|do|call|send|get|let|try|take|check|make|give|have|need|talk|meet|go|come|bring|handle|text|email|write|look|find|pick|grab|follow|update|review|start|finish|figure|ask|tell|keep|work|fix|wait|pay|just|also|never|always|probably|definitely|stop|leave|catch|ping|share|reach|set|put|add|run|test|read|show|think|say|hold|help|join|visit|order|book|pass|forward|loop|drop|swing)\b"
        ),
        &t,
        |c| format!("I'll{}{}", grp(c, 2), grp(c, 3)),
    );
    t = replace_all(re!(r"(?i)\balot\b"), &t, |c| match_case(grp(c, 0), "a lot"));
    if o.kind != "code" {
        t = polish_punctuation(&t);
    }
    if o.expand {
        t = replace_all(re!(r"(?i)\bcan't\b"), &t, |c| match_case(grp(c, 0), "cannot"));
        t = replace_all(re!(r"(?i)\bwon't\b"), &t, |c| match_case(grp(c, 0), "will not"));
        t = replace_all(
            re!(r"(?i)\b(do|does|did|is|are|was|were|has|have|had|should|would|could)n't\b"),
            &t,
            |c| match_case(grp(c, 0), &format!("{} not", grp(c, 1))),
        );
        t = replace_all(re!(r"\bI'm\b"), &t, |c| match_case(grp(c, 0), "I am"));
        t = replace_all(re!(r"\bI've\b"), &t, |c| match_case(grp(c, 0), "I have"));
        t = replace_all(re!(r"\bI'll\b"), &t, |c| match_case(grp(c, 0), "I will"));
        t = replace_all(re!(r"(?i)\b(you|we|they)'re\b"), &t, |c| {
            match_case(grp(c, 0), &format!("{} are", grp(c, 1)))
        });
        t = replace_all(re!(r"(?i)\b(you|we|they)'ve\b"), &t, |c| {
            match_case(grp(c, 0), &format!("{} have", grp(c, 1)))
        });
    }
    if o.formalize {
        for (re, to) in formal_swaps().iter() {
            t = replace_all(re, &t, |c| match_case(grp(c, 0), to));
        }
    }
    t = replace_all(re!(r"(?i)\b(a|an)\s+([A-Za-z][A-Za-z'-]*)"), &t, |c| {
        let (art, word, m) = (grp(c, 1), grp(c, 2), grp(c, 0));
        match an_needed(word) {
            Some(true) if art.eq_ignore_ascii_case("a") => {
                format!("{} {}", if art == "A" { "An" } else { "an" }, word)
            }
            Some(false) if art.eq_ignore_ascii_case("an") => {
                format!("{} {}", if art == "An" { "A" } else { "a" }, word)
            }
            _ => m.to_string(),
        }
    });
    let trimmed = t.trim_end().to_string();
    let last_line = trimmed.rsplit('\n').next().unwrap_or("");
    let is_list_end = is_match(re!(r"^\s*(?:[-*]|\d+\.)\s"), last_line);
    let signature = trimmed.contains('\n') && word_count(last_line) <= 3;
    if o.kind != "code"
        && o.terminal_punct != Some(false)
        && !is_list_end
        && !signature
        && word_count(&trimmed) >= 4
        && is_match(re!(r"[\p{L}\p{N}]$"), &trimmed)
    {
        let single = split_sentences(&trimmed).len() == 1;
        let question = single
            && is_match(
                re!(
                    r"(?i)^(?:who|what|when|where|why|how|can you|could you|would you|will you|do you|does it|did you|are you|is it|is there|are there|should we|shall we)\b"
                ),
                &trimmed,
            );
        return format!("{trimmed}{}", if question { "?" } else { "." });
    }
    t
}

fn formal_swaps() -> &'static Vec<(Regex, &'static str)> {
    static V: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"(?i)\bgonna\b", "going to"),
            (r"(?i)\bwanna\b", "want to"),
            (r"(?i)\bgotta\b", "have to"),
            (r"(?i)\bkinda\b", "kind of"),
            (r"(?i)\bsorta\b", "sort of"),
            (r"(?i)\blemme\b", "let me"),
            (r"(?i)\bgimme\b", "give me"),
        ]
        .iter()
        .map(|(p, t)| (Regex::new(p).unwrap(), *t))
        .collect()
    });
    &V
}

fn drops() -> &'static Vec<Regex> {
    static V: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        [
        r"(?i)\bI hope (?:this|that) (?:email|message|note) finds you well[.!,]?\s*",
        r"(?i)\bI hope you(?:'re| are) (?:doing )?(?:well|great|good)[.!,]?\s*",
        r"(?im)^(?:certainly|absolutely|of course|great question)[!,.]\s+",
        r"(?i)\bit(?:'s| is) (?:important|worth|crucial|essential) to (?:note|mention|remember|highlight) that,?\s*",
        r"(?i)\bit(?:'s| is) worth noting that,?\s*",
        r"(?i)\bneedless to say,?\s*",
        r"(?i)\bin today(?:'s| s)? (?:fast-paced |ever-changing |digital )?(?:world|landscape|environment),?\s*",
        r"(?i)\bin conclusion,?\s*",
        r"(?i)\bI wanted to take a moment to\s+",
        r"(?i)\bI would like to take this opportunity to\s+",
        r"(?i)\bas an AI(?: language model)?,?\s*",
    ].iter().map(|p| Regex::new(p).unwrap()).collect()
    });
    &V
}
fn swaps() -> &'static Vec<(Regex, &'static str)> {
    static V: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"(?i)\butiliz(?:e|ing)\b", "use"),
            (r"(?i)\butilizes\b", "uses"),
            (r"(?i)\butilized\b", "used"),
            (r"(?i)\bin order to\b", "to"),
            (r"(?i)\bdue to the fact that\b", "because"),
            (r"(?i)\bat this point in time\b", "now"),
            (r"(?i)\ba plethora of\b", "plenty of"),
            (r"(?i)\ba myriad of\b", "lots of"),
            (r"(?i)\bdelves into\b", "digs into"),
            (r"(?i)\bdelve into\b", "dig into"),
            (r"(?i)\bdelving into\b", "digging into"),
            (r"(?i)\bwith regard to\b", "about"),
        ]
        .iter()
        .map(|(p, t)| (Regex::new(p).unwrap(), *t))
        .collect()
    });
    &V
}
fn strong() -> &'static Vec<(Regex, &'static str)> {
    static V: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"(?i)\b(?:furthermore|moreover|additionally),\s*", "Also, "),
            (r"(?i)\bcommence\b", "start"),
            (r"(?i)\bendeavou?r\b", "try"),
            (r"(?i)\bfacilitate\b", "help"),
            (r"(?i)\bseamlessly\b", "smoothly"),
            (r"(?i)\bseamless\b", "smooth"),
            (r"(?i)\bpivotal\b", "key"),
            (r"(?i)\bnumerous\b", "many"),
            (r"(?i)\bapproximately\b", "about"),
            (
                r"(?i)\bplease (?:do not|don't) hesitate to (?:reach out|contact me)\b",
                "let me know if you have questions",
            ),
        ]
        .iter()
        .map(|(p, t)| (Regex::new(p).unwrap(), *t))
        .collect()
    });
    &V
}
fn contract() -> &'static Vec<(Regex, &'static str)> {
    static V: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"(?i)\bdo not\b", "don't"),
            (r"(?i)\bdoes not\b", "doesn't"),
            (r"(?i)\bdid not\b", "didn't"),
            (r"(?i)\bis not\b", "isn't"),
            (r"(?i)\bare not\b", "aren't"),
            (r"(?i)\bcannot\b", "can't"),
            (r"(?i)\bwill not\b", "won't"),
            (r"(?i)\bwould not\b", "wouldn't"),
            (r"(?i)\bshould not\b", "shouldn't"),
            (r"(?i)\bcould not\b", "couldn't"),
        ]
        .iter()
        .map(|(p, t)| (Regex::new(p).unwrap(), *t))
        .collect()
    });
    &V
}

pub fn humanize(text: &str, level: &str, formalize: bool) -> String {
    if level == "off" {
        return text.to_string();
    }
    let mut t = text.to_string();
    for re in drops().iter() {
        t = replace_lit(re, &t, "");
    }
    for (re, to) in swaps().iter() {
        t = replace_all(re, &t, |c| match_case(grp(c, 0), to));
    }
    if level == "strong" {
        t = replace_lit(re!(r"\s*[—–]\s*"), &t, ", ");
        for (re, to) in strong().iter() {
            t = replace_all(re, &t, |c| match_case(grp(c, 0), to));
        }
        if !formalize {
            for (re, to) in contract().iter() {
                t = replace_all(re, &t, |c| match_case(grp(c, 0), to));
            }
            t = replace_lit(re!(r"\bI am\b(?!\s*[.,!?])"), &t, "I'm");
            t = replace_lit(re!(r"\bI will\b(?!\s*[.,!?])"), &t, "I'll");
        }
    }
    replace_lit(re!(r" {2,}"), &capitalize_sentences(&t), " ")
}

pub fn exclude_chars(text: &str, list: &[String]) -> String {
    if list.is_empty() {
        return text.to_string();
    }
    let mut t = text.to_string();
    for raw in list {
        let it = raw.trim();
        if it.is_empty() {
            continue;
        }
        t = match it {
            "—" | "–" | "—–" | "dash" | "em dash" | "en dash" => {
                replace_lit(re!(r"[ \t]*[—–][ \t]*"), &t, ", ")
            }
            "-" | "hyphen" => replace_lit(re!(r"(?<=\w)-(?=\w)"), &t, " "),
            "…" | "ellipsis" => t.replace('…', "..."),
            ";" => t.replace(';', ","),
            "!" => t.replace('!', "."),
            ":" => replace_lit(re!(r":(?!\n|$)"), &t, ","),
            "“" | "”" | "‘" | "’" | "curly quotes" => {
                t.replace(['“', '”'], "\"").replace(['‘', '’'], "'")
            }
            "emoji" | "emojis" => {
                replace_lit(re!(r"[\x{1F000}-\x{1FAFF}\x{2600}-\x{27BF}\x{FE0F}\x{200D}]"), &t, "")
            }
            other if other.chars().any(|c| c.is_alphanumeric()) => {
                match Regex::new(&format!(
                    r"(?i)(?<![\p{{L}}\p{{N}}']){}(?![\p{{L}}\p{{N}}'])",
                    fancy_regex::escape(other)
                )) {
                    Ok(re) => replace_lit(&re, &t, ""),
                    Err(_) => t,
                }
            }
            other => t.replace(other, ""),
        };
    }
    let removed_words = t != text && list.iter().any(|x| x.chars().any(|c| c.is_alphanumeric()));
    t = replace_lit(re!(r"[ \t]{2,}"), &t, " ");
    t = replace_all(re!(r"[ \t]+([,.;:!?])"), &t, |c| grp(c, 1).to_string());
    t = replace_lit(re!(r",\s*,"), &t, ",");
    t = replace_all(re!(r",\s*([.!?])"), &t, |c| grp(c, 1).to_string());
    t = replace_lit(re!(r"(?m)[ \t]+$"), &t, "");
    if removed_words {
        t = replace_all(re!(r"([.!?])(?:\s*[.!?])+"), &t, |c| grp(c, 1).to_string());
        t = replace_lit(re!(r"\A\s*[.,;:!?]+\s*"), &t, "");
        t = replace_all(re!(r"(?m)(^|[.!?]\s+),\s*"), &t, |c| grp(c, 1).to_string());
        t = replace_all(re!(r"([.!?]\s+)(\p{Ll})"), &t, |c| {
            format!("{}{}", grp(c, 1), grp(c, 2).to_uppercase())
        });
        if text.chars().next().is_some_and(|c| c.is_uppercase()) {
            t = cap_first(t.trim_start());
        }
    }
    t.trim().to_string()
}

/// Capitalize days, months and greeting/sign-off names, conservatively.
pub fn proper_nouns(text: &str) -> String {
    let mut t = replace_all(
        re!(
            r"(?i)\b(monday|tuesday|wednesday|thursday|friday|saturday|sunday|january|february|april|june|july|august|september|october|november|december)\b"
        ),
        text,
        |c| cap_first(&grp(c, 1).to_lowercase()),
    );
    t = replace_all(
        re!(
            r"(?i)\b(march|may)\b(?=\s+(?:\d|first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth|eleventh|twelfth|thirteenth|fourteenth|fifteenth|sixteenth|seventeenth|eighteenth|nineteenth|twentieth|thirtieth))"
        ),
        &t,
        |c| cap_first(&grp(c, 1).to_lowercase()),
    );
    const NOT_NAMES: &[&str] = &[
        "there",
        "everyone",
        "everybody",
        "all",
        "guys",
        "team",
        "folks",
        "friend",
        "friends",
        "man",
        "dude",
        "mate",
        "buddy",
        "again",
        "you",
        "me",
        "him",
        "her",
        "them",
        "the",
        "a",
        "an",
        "that",
        "this",
        "it",
        "so",
        "very",
        "much",
        "for",
        "to",
        "in",
        "on",
        "at",
        "and",
        "but",
        "or",
        "i",
        "we",
        "he",
        "she",
        "they",
        "is",
        "are",
        "was",
        "were",
        "be",
        "will",
        "would",
        "can",
        "could",
        "should",
        "just",
        "really",
        "how",
        "what",
        "when",
        "where",
        "why",
        "who",
        "do",
        "did",
        "does",
        "not",
        "no",
        "yes",
        "ok",
        "okay",
        "please",
        "thanks",
        "thank",
        "sorry",
        "my",
        "your",
        "our",
        "their",
        "some",
        "any",
        "one",
        "here",
        "now",
        "then",
        "well",
        "look",
        "listen",
        "wait",
        "stop",
        "come",
        "go",
        "let",
        "lets",
        "see",
        "hi",
        "hey",
        "hello",
        "dear",
        "sir",
        "madam",
        "maam",
        "anyone",
        "someone",
        "world",
        "there's",
        "everything",
        "nothing",
        "something",
        "good",
        "great",
        "nice",
        "sure",
        "right",
        "yeah",
        "yep",
        "nope",
    ];
    const AFTER_NAME: &[&str] = &[
        "can", "could", "would", "will", "do", "did", "are", "is", "i", "we", "thanks", "thank", "just",
        "how", "what", "when", "where", "please", "hope", "wanted", "have", "has", "quick", "sorry", "it",
        "this", "that", "the", "hey", "hello", "hi", "good", "welcome", "great", "so", "also", "and", "my",
        "your",
    ];
    t = replace_all(
        re!(r"(?i)\b(hi|hey|hello|dear)(\s+)([a-z]{2,})(?=\s*(?:[,.!?\n]|$)|\s+([a-z']+)\b)"),
        &t,
        |c| {
            let (greet, gap, name) = (grp(c, 1), grp(c, 2), grp(c, 3));
            let next = c.get(4).map(|m| m.as_str().to_lowercase());
            let ok = !NOT_NAMES.contains(&name.to_lowercase().as_str())
                && next.as_deref().map_or(true, |n| AFTER_NAME.contains(&n));
            if ok {
                format!("{greet}{gap}{}", cap_first(&name.to_lowercase()))
            } else {
                grp(c, 0).to_string()
            }
        },
    );
    t = replace_all(
        re!(
            r"\b(Hi|Hey|Hello)\s+([A-Z][a-z]+)\s+(can|could|would|will|thanks|thank|just|how|what|please|i|we|hope)\b"
        ),
        &t,
        |c| format!("{} {}, {}", grp(c, 1), grp(c, 2), grp(c, 3)),
    );
    t = replace_all(re!(r"(?m)^(Dear|Hi|Hello|Hey)\s+([A-Z][a-z]+)$"), &t, |c| {
        format!("{} {},", grp(c, 1), grp(c, 2))
    });
    t = replace_all(
        re!(
            r"(?i)\b(best regards|kind regards|warm regards|regards|sincerely|cheers)(\s+)([a-z]{2,})([.!]?)\s*$"
        ),
        &t,
        |c| {
            let name = grp(c, 3).to_lowercase();
            if NOT_NAMES.contains(&name.as_str()) {
                grp(c, 0).to_string()
            } else {
                format!("{}{}{}{}", grp(c, 1), grp(c, 2), cap_first(&name), grp(c, 4))
            }
        },
    );
    t
}

/// Normalize model lists: "- " bullets, renumbered runs, tight items, blank line around.
pub fn normalize_lists(text: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        Text,
        Blank,
        Bullet,
        Number,
    }
    let bullet = re!(r"^(\s*)[•·▪◦*-]\s+(\S.*)$");
    let number = re!(r"^(\s*)(\d{1,2})[.)]\s+(\S.*)$");
    let mut rows: Vec<(Kind, String, String)> = vec![]; // (kind, indent, content)
    for line in text.split('\n') {
        let line = line.trim_end();
        if line.trim().is_empty() {
            rows.push((Kind::Blank, String::new(), String::new()));
        } else if let Some(c) = bullet.captures(line).ok().flatten() {
            rows.push((Kind::Bullet, grp(&c, 1).to_string(), grp(&c, 2).to_string()));
        } else if let Some(c) = number.captures(line).ok().flatten() {
            rows.push((Kind::Number, grp(&c, 1).to_string(), format!("{}\u{0}{}", grp(&c, 2), grp(&c, 3))));
        } else {
            rows.push((Kind::Text, String::new(), line.to_string()));
        }
    }
    for i in 0..rows.len() {
        if rows[i].0 == Kind::Number {
            let prev = i > 0 && matches!(rows[i - 1].0, Kind::Number | Kind::Bullet);
            let next = i + 1 < rows.len() && matches!(rows[i + 1].0, Kind::Number | Kind::Bullet);
            let kind_at = |j: isize| usize::try_from(j).ok().and_then(|j| rows.get(j).map(|r| r.0));
            let across = |d: isize| {
                let j = i as isize + d;
                kind_at(j) == Some(Kind::Blank) && kind_at(j + d) == Some(Kind::Number)
            };
            if !prev && !next && !across(-1) && !across(1) {
                let (n, body) = rows[i]
                    .2
                    .split_once('\u{0}')
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .unwrap_or_default();
                rows[i] = (Kind::Text, String::new(), format!("{}{}. {}", rows[i].1, n, body));
            }
        }
    }
    let mut keep: Vec<bool> = vec![true; rows.len()];
    for i in 1..rows.len().saturating_sub(1) {
        let is_list = |k: Kind| k == Kind::Bullet || k == Kind::Number;
        if rows[i].0 == Kind::Blank && is_list(rows[i - 1].0) && rows[i - 1].0 == rows[i + 1].0 {
            keep[i] = false;
        }
    }
    let rows: Vec<&(Kind, String, String)> =
        rows.iter().enumerate().filter(|(i, _)| keep[*i]).map(|(_, r)| r).collect();
    let mut out: Vec<String> = vec![];
    let mut counter: Vec<(String, usize)> = vec![]; // (indent, last number) for the numbered run in progress
    for (i, (kind, indent, content)) in rows.iter().enumerate() {
        let in_list = |k: Kind| k == Kind::Bullet || k == Kind::Number;
        if in_list(*kind)
            && i > 0
            && !in_list(rows[i - 1].0)
            && rows[i - 1].0 == Kind::Text
            && !out.last().is_some_and(|l| l.trim_end().ends_with(':'))
        {
            out.push(String::new());
        }
        if *kind == Kind::Text && i > 0 && in_list(rows[i - 1].0) {
            out.push(String::new());
        }
        match kind {
            Kind::Bullet => {
                counter.retain(|(ind, _)| ind.len() < indent.len());
                out.push(format!("{indent}- {content}"));
            }
            Kind::Number => {
                let body = content.split_once('\u{0}').map(|(_, b)| b).unwrap_or(content);
                counter.retain(|(ind, _)| ind.len() <= indent.len()); // a new item at this level restarts any nested count
                let n = match counter.iter_mut().find(|(ind, _)| ind == indent) {
                    Some(c) => {
                        c.1 += 1;
                        c.1
                    }
                    None => {
                        counter.push((indent.clone(), 1));
                        1
                    }
                };
                out.push(format!("{indent}{n}. {body}"));
            }
            Kind::Blank => {
                counter.clear();
                out.push(String::new());
            }
            Kind::Text => {
                counter.clear();
                out.push(content.clone());
            }
        }
    }
    out.join("\n").trim_matches('\n').to_string()
}

#[cfg(test)]
mod list_tests {
    use super::*;

    #[test]
    fn a_spoken_comma_list_becomes_a_list_and_its_tail_joins_the_lead_in() {
        assert_eq!(auto_list("I'm going to need milk, eggs, and cheese for the trip."), "For the trip, I'm going to need:
- Milk
- Eggs
- Cheese");
        assert_eq!(auto_list("I need milk, eggs, and cheese."), "I need:
- Milk
- Eggs
- Cheese");
        assert_eq!(auto_list("We went to the store, bought milk, and then went home."), "We went to the store, bought milk, and then went home.");
        // talk with commas is not a list
        for s in ["Oh, like whisper flower, something else.", "Well, I think, maybe tomorrow.", "Yeah, so, anyway, let's go.", "Hmm, okay, sounds good."] {
            assert_eq!(auto_list(s), s);
        }
        assert_eq!(auto_list("Apples, oranges, pears."), "- Apples\n- Oranges\n- Pears");
    }
}
