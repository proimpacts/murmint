//! The spoken language, and vocabulary hints for the speech model.

use serde_json::Value;

/// The spoken language: whatever was picked, or auto-detect.
pub fn language(cfg: &Value) -> String {
    cfg["dictation"]["language"].as_str().unwrap_or("auto").to_string()
}

/// Vocabulary only, and skipped for very short clips.
pub fn stt_prompt(cfg: &Value, duration_sec: f64) -> String {
    if duration_sec < 4.0 || cfg["privacy"]["sendVocabulary"] == serde_json::json!(false) {
        return String::new();
    }
    crate::learn::stt_prompt(&cfg["personal"])
}

/// Removes prompt text the speech model repeated back.
pub fn strip_echo(text: &str, prompt: &str) -> String {
    let mut t = text.to_string();
    if !prompt.is_empty() {
        t = t.replace(prompt, "");
    }
    t = crate::re!(r"(?i)\s*vocabulary:[^.\n]*\.?").replace_all(&t, "").into_owned();
    t.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_language_is_the_one_picked_or_auto() {
        assert_eq!(language(&json!({ "dictation": { "language": "fr" } })), "fr");
        assert_eq!(language(&json!({ "dictation": { "language": "auto" } })), "auto");
        assert_eq!(language(&json!({})), "auto", "an old config without the setting still works");
    }

    #[test]
    fn the_speech_prompt_is_vocabulary_only() {
        let mut c = json!({ "personal": { "vocabulary": [] }, "privacy": {} });
        assert_eq!(stt_prompt(&c, 6.0), "", "no vocabulary, no prompt");
        c["personal"] = json!({ "vocabulary": [{ "id": "1", "term": "Kubernetes", "sounds": [], "note": "", "trained": true }] });
        assert!(stt_prompt(&c, 8.0).contains("Kubernetes"));
        assert_eq!(stt_prompt(&c, 2.0), "", "not for very short recordings");
        c["privacy"] = json!({ "sendVocabulary": false });
        assert_eq!(stt_prompt(&c, 8.0), "");
    }

    #[test]
    fn echoed_prompts_are_removed() {
        assert_eq!(strip_echo("Vocabulary: Kubernetes, Murmint. deploy it", "Vocabulary: Kubernetes, Murmint."), "deploy it");
        assert_eq!(strip_echo("my vocabulary is small", ""), "my vocabulary is small");
    }
}
