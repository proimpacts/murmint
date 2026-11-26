//! Advanced prompting: the last few messages of the person's coding-agent chat (Claude Code or Codex, in the
//! terminal or their VS Code / Cursor extensions), read from the transcripts those tools keep on this PC. Only what
//! was said and the names of files the agent touched are used, never file contents, code, or tool output.

use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const TURNS: usize = 3;
const RECENT: Duration = Duration::from_secs(30 * 60);
const TAIL_BYTES: u64 = 512 * 1024;
const MESSAGE_CHARS: usize = 500;
const MAX_FILES: usize = 8;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Agent {
    Claude,
    Codex,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

fn chats_in(dir: &Path, depth: usize, agent: Agent, out: &mut Vec<(SystemTime, Agent, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                chats_in(&p, depth - 1, agent, out);
            }
        } else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                if SystemTime::now().duration_since(t).unwrap_or_default() < RECENT {
                    out.push((t, agent, p));
                }
            }
        }
    }
}

/// Claude Code and Codex chats changed in the last half hour, newest first.
fn recent_chats() -> Vec<(SystemTime, Agent, PathBuf)> {
    let Some(h) = home() else { return vec![] };
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| h.join(".claude"));
    let codex = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| h.join(".codex"));
    let mut out = vec![];
    chats_in(&claude.join("projects"), 1, Agent::Claude, &mut out);
    chats_in(&codex.join("sessions"), 3, Agent::Codex, &mut out);
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// The project folder a chat was run in: both tools record the working folder on their lines.
fn project_of(jsonl: &str) -> Option<String> {
    jsonl.lines().rev().find_map(|line| {
        let v: Value = serde_json::from_str(line).ok()?;
        let cwd = v["cwd"].as_str().or_else(|| v["payload"]["cwd"].as_str())?;
        let name = cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next()?.to_string();
        (!name.is_empty()).then_some(name)
    })
}

/// Does the window title name this project? Editors show it ("main.rs - murmint - Visual Studio Code"),
/// and so do terminals opened in it.
fn title_names(title: &str, project: &str) -> bool {
    let (t, p) = (title.to_lowercase(), project.to_lowercase());
    if p.len() < 2 {
        return false;
    }
    t.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c == '.')).any(|w| w == p)
}

/// The chat for the window being typed into: the newest chat from the project the window shows, or, when the
/// window doesn't name a project, the only recent chat. With several recent chats and no match, none is used,
/// so context from the wrong chat never gets in.
fn pick(title: &str, chats: &[(Agent, String)], projects: &[Option<String>]) -> Option<usize> {
    if let Some(i) = projects.iter().position(|p| p.as_deref().is_some_and(|p| title_names(title, p))) {
        return Some(i);
    }
    (chats.len() == 1).then_some(0)
}

/// Only the end of the file: transcripts grow to many megabytes.
fn tail(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).ok()?;
    let mut buf = vec![];
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).to_string();
    // the first line is usually cut in half
    Some(if len > TAIL_BYTES { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text })
}

fn clean(text: &str) -> String {
    let no_code = crate::re!(r"(?s)```.*?```").replace_all(text, " [code] ").to_string();
    let flat = no_code.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > MESSAGE_CHARS {
        format!("{}…", flat.chars().take(MESSAGE_CHARS).collect::<String>())
    } else {
        flat
    }
}

/// Just the file name, so no folder structure leaves the PC.
fn file_name(path: &str) -> Option<String> {
    let name = path.trim().trim_matches('"').rsplit(['/', '\\']).next()?.to_string();
    (name.contains('.') && name.len() <= 80 && !name.contains(' ')).then_some(name)
}

/// File names in an agent's tool call: its path fields, or the files an apply_patch names. Never the contents.
fn files_in_call(input: &Value) -> Vec<String> {
    let mut out = vec![];
    for k in ["file_path", "path", "notebook_path"] {
        if let Some(n) = input[k].as_str().and_then(file_name) {
            out.push(n);
        }
    }
    if let Some(patch) = input["input"].as_str().or_else(|| input["patch"].as_str()) {
        for line in patch.lines() {
            if let Some(p) = line.strip_prefix("*** Update File: ").or_else(|| line.strip_prefix("*** Add File: ")) {
                out.extend(file_name(p));
            }
        }
    }
    out
}

enum Item {
    Said(&'static str, String),
    File(String),
}

fn text_blocks(v: &Value, kinds: &[&str]) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| kinds.iter().any(|k| b["type"] == *k))
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn claude_items(v: &Value) -> Vec<Item> {
    if v["isSidechain"] == true || v["isMeta"] == true || v["isCompactSummary"] == true {
        return vec![];
    }
    let content = &v["message"]["content"];
    let mut out = vec![];
    if let Some(blocks) = content.as_array() {
        for b in blocks.iter().filter(|b| b["type"] == "tool_use") {
            out.extend(files_in_call(&b["input"]).into_iter().map(Item::File));
        }
    }
    let who = match v["type"].as_str() {
        Some("user") => "You",
        Some("assistant") => "AI",
        _ => return out,
    };
    out.push(Item::Said(who, text_blocks(content, &["text"])));
    out
}

fn codex_items(v: &Value) -> Vec<Item> {
    let p = &v["payload"];
    if v["type"] != "response_item" {
        return vec![];
    }
    match p["type"].as_str() {
        Some("message") => {
            let who = match p["role"].as_str() {
                Some("user") => "You",
                Some("assistant") => "AI",
                _ => return vec![],
            };
            vec![Item::Said(who, text_blocks(&p["content"], &["input_text", "output_text", "text"]))]
        }
        Some("function_call") | Some("custom_tool_call") => {
            let args = p["arguments"].as_str().and_then(|a| serde_json::from_str::<Value>(a).ok()).unwrap_or_else(|| {
                // apply_patch sends the patch as plain text
                serde_json::json!({ "input": p["input"].as_str().or(p["arguments"].as_str()).unwrap_or("") })
            });
            files_in_call(&args).into_iter().map(Item::File).collect()
        }
        _ => vec![],
    }
}

/// The last few exchanges and the files touched during them.
fn summarize(agent: Agent, jsonl: &str) -> Option<String> {
    let mut said: Vec<(&'static str, String)> = vec![];
    let mut files: Vec<(usize, String)> = vec![];
    for line in jsonl.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let items = if agent == Agent::Claude { claude_items(&v) } else { codex_items(&v) };
        for item in items {
            match item {
                Item::File(f) => files.push((said.len(), f)),
                Item::Said(who, text) => {
                    // the tools' own notices and context arrive as tagged text, not as something anyone wrote
                    let text = text.trim();
                    if text.is_empty() || text.starts_with('<') {
                        continue;
                    }
                    match said.last_mut() {
                        Some((last, words)) if *last == who => {
                            words.push(' ');
                            words.push_str(text);
                        }
                        _ => said.push((who, text.to_string())),
                    }
                }
            }
        }
    }
    let start = said.len().saturating_sub(TURNS * 2);
    let mut names: Vec<String> = vec![];
    for (at, f) in files.iter().rev() {
        if *at + 1 >= start && !names.contains(f) && names.len() < MAX_FILES {
            names.push(f.clone());
        }
    }
    let lines: Vec<String> =
        said[start..].iter().map(|(who, words)| format!("{who}: {}", clean(words))).filter(|l| l.len() > 5).collect();
    if lines.is_empty() {
        return None;
    }
    let tool = if agent == Agent::Claude { "Claude Code" } else { "Codex" };
    let files_line = if names.is_empty() { String::new() } else { format!("Files it worked on: {}\n", names.join(", ")) };
    Some(format!("From {tool}.\n{files_line}{}", lines.join("\n")))
}

/// The end of the coding-agent chat that belongs to the window being typed into, ready for the formatting model.
pub fn recent(window_title: &str) -> Option<String> {
    let found = recent_chats();
    let texts: Vec<(Agent, String)> =
        found.iter().filter_map(|(_, agent, path)| tail(path).map(|t| (*agent, t))).collect();
    let projects: Vec<Option<String>> = texts.iter().map(|(_, t)| project_of(t)).collect();
    let (agent, text) = &texts[pick(window_title, &texts, &projects)?];
    summarize(*agent, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_gives_words_and_file_names_only() {
        let jsonl = [
            r#"{"type":"user","message":{"role":"user","content":"make the save button blue"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"tool_use","name":"Edit","input":{"file_path":"C:\\proj\\src\\settings.css","old_string":"SECRET CODE","new_string":"x"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"whole file contents"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done:\n```css\n.save { color: blue }\n```\nin settings.css."}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/model</command-name>"}}"#,
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"a helper agent's task"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"now make it bigger"}}"#,
        ]
        .join("\n");
        let s = summarize(Agent::Claude, &jsonl).unwrap();
        assert!(s.starts_with("From Claude Code.\nFiles it worked on: settings.css\n"), "{s}");
        assert!(s.contains("You: make the save button blue") && s.contains("AI: Done: [code] in settings.css.") && s.contains("You: now make it bigger"), "{s}");
        for leaked in ["SECRET CODE", "whole file contents", "color: blue", "proj", "/model", "helper agent"] {
            assert!(!s.contains(leaked), "{leaked} leaked: {s}");
        }
    }

    #[test]
    fn codex_gives_words_and_patched_file_names() {
        let jsonl = [
            r#"{"type":"session_meta","payload":{"cwd":"C:\\proj"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"system rules"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"fix the login bug"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call","name":"apply_patch","arguments":"{\"input\":\"*** Begin Patch\\n*** Update File: src/auth/login.ts\\n-old\\n+new\\n*** End Patch\"}"}}"#,
            r#"{"type":"response_item","payload":{"type":"function_call_output","output":"file contents here"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Fixed the token check in login.ts."}]}}"#,
        ]
        .join("\n");
        let s = summarize(Agent::Codex, &jsonl).unwrap();
        assert!(s.starts_with("From Codex.\nFiles it worked on: login.ts\n"), "{s}");
        assert!(s.contains("You: fix the login bug") && s.contains("AI: Fixed the token check in login.ts."), "{s}");
        for leaked in ["system rules", "file contents here", "-old", "proj"] {
            assert!(!s.contains(leaked), "{leaked} leaked: {s}");
        }
    }

    #[test]
    fn the_chat_is_the_one_for_the_window_being_typed_into() {
        let chat = |cwd: &str| format!(r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":"hi"}}}}"#);
        let codex = r#"{"type":"session_meta","payload":{"cwd":"D:\\work\\shop-site"}}"#.to_string();
        let texts = vec![
            (Agent::Claude, chat("E:\\\\vcw")),
            (Agent::Codex, codex),
            (Agent::Claude, chat("E:\\\\murmint-website")),
        ];
        let projects: Vec<Option<String>> = texts.iter().map(|(_, t)| project_of(t)).collect();
        assert_eq!(projects, [Some("vcw".into()), Some("shop-site".into()), Some("murmint-website".into())]);
        // the window names the project, even when another chat is newer
        assert_eq!(pick("settings.rs - murmint-website - Visual Studio Code", &texts, &projects), Some(2));
        assert_eq!(pick("index.ts - shop-site - Cursor", &texts, &projects), Some(1));
        // a project name inside another word doesn't count
        assert_eq!(pick("vcwtools - Visual Studio Code", &texts, &projects), None);
        // several recent chats and nothing to match: none is used rather than a guess
        assert_eq!(pick("Windows PowerShell", &texts, &projects), None);
        // a single recent chat is used even when the title doesn't say
        assert_eq!(pick("Windows PowerShell", &texts[..1], &projects[..1]), Some(0));
    }

    #[test]
    fn only_the_last_few_exchanges_are_kept() {
        let mut lines = vec![];
        for i in 0..10 {
            lines.push(format!(r#"{{"type":"user","message":{{"role":"user","content":"question {i}"}}}}"#));
            lines.push(format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"answer {i}"}}]}}}}"#));
        }
        let s = summarize(Agent::Claude, &lines.join("\n")).unwrap();
        assert!(!s.contains("question 6") && s.contains("question 7") && s.contains("answer 9"), "{s}");
        assert!(clean(&"word ".repeat(400)).chars().count() <= MESSAGE_CHARS + 1);
    }
}
