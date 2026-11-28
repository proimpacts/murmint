use crate::re;
use serde_json::{json, Value};

pub fn presets() -> Value {
    serde_json::from_str(include_str!("../data/presets.json")).unwrap_or(json!([]))
}

/// Which Models section a model belongs in: "stt" for speech to text, "chat" for formatting (text in, text out), or
/// None for everything else (images, video, voices, music, embeddings, live audio, agents), which is left out of both.
pub fn kind_of(id: &str) -> Option<&'static str> {
    let id = id.to_lowercase();
    // speech to text first, so "gpt-4o-transcribe" or "gemini-transcribe-live" isn't thrown out as audio or live
    let stt = re!(r"whisper|transcri|voxtral|parakeet|canary|sensevoice|moonshine|\bstt\b|speech-to-text|\basr\b|deepgram|chirp");
    if stt.is_match(&id).unwrap_or(false) {
        return Some("stt");
    }
    let neither = re!(concat!(
        r"embed|rerank|moderation|guard|safeguard|classif",                    // not for writing
        r"|\btts\b|text-to-speech|orpheus|playai|voice|speech|lyria|music|audio", // makes sound
        r"|native-audio|realtime|\blive\b|computer-use|deep-research",          // live sessions and agents
        r"|image|imagen|dall-?e|flux|diffusion|stable-|sdxl|recraft|ideogram|seedream|seededit|kolors|hidream|playground|midjourney", // pictures
        r"|video|veo|sora|kling|\bwan[-0-9]|hailuo|runway|luma",               // video
        r"|\bocr\b|-ocr"
    ));
    if neither.is_match(&id).unwrap_or(false) {
        return None;
    }
    Some("chat")
}

pub fn parse_model_list(json: &Value, free: bool) -> Vec<Value> {
    let rows: Vec<Value> = if let Some(a) = json.as_array() {
        a.clone()
    } else {
        json["data"].as_array().or_else(|| json["models"].as_array()).cloned().unwrap_or_default()
    };
    let mut out: Vec<Value> = vec![];
    for r in rows {
        let id = if let Some(s) = r.as_str() {
            s.to_string()
        } else {
            r["id"].as_str().or_else(|| r["name"].as_str()).unwrap_or("").to_string()
        };
        let Some(task) = (if id.is_empty() { None } else { kind_of(&id) }) else { continue };
        // Gemini lists say what a model can do: no generateContent means no text out
        if let Some(methods) = r["supportedGenerationMethods"].as_array() {
            if !methods.iter().any(|m| m == "generateContent") {
                continue;
            }
        }
        if let Some(mods) = r["architecture"]["output_modalities"].as_array() {
            if !mods.iter().any(|m| m == "text") || (task == "chat" && mods.iter().any(|m| m == "image" || m == "audio")) {
                continue;
            }
        }
        let name = r["display_name"].as_str().or_else(|| r["name"].as_str()).filter(|n| !n.is_empty());
        let label = name.filter(|n| *n != id).unwrap_or(&id).trim_start_matches("models/").to_string();
        let mut m = json!({ "id": id, "label": label, "task": task, "tier": "decent" });
        let p = &r["pricing"];
        let prompt =
            p["prompt"].as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| p["prompt"].as_f64());
        if free {
            m["price"] =
                if task == "stt" { json!({ "perMinute": 0 }) } else { json!({ "inPerM": 0, "outPerM": 0 }) };
        } else if let (Some(i), "chat") = (prompt, task) {
            let o = p["completion"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| p["completion"].as_f64())
                .unwrap_or(i)
                * 1e6;
            let i = i * 1e6;
            let round = |x: f64| (x * 10000.0).round() / 10000.0;
            m["price"] = json!({ "inPerM": round(i), "outPerM": round(o) });
            // "best" is never guessed: it is kept for the one top model per provider
            m["tier"] = json!(if o <= 0.2 {
                "worst"
            } else if o <= 0.6 {
                "bad"
            } else if o <= 4.0 {
                "decent"
            } else {
                "good"
            });
        }
        out.push(m);
    }
    out.sort_by(|a, b| {
        a["id"].as_str().unwrap_or("").to_lowercase().cmp(&b["id"].as_str().unwrap_or("").to_lowercase())
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_model_lands_in_the_right_section_or_none() {
        for id in ["whisper-large-v3-turbo", "gpt-4o-mini-transcribe", "gpt-live-transcribe", "models/gemini-3.5-transcribe-live", "@cf/deepgram/nova-3", "distil-whisper-large-v3-en", "voxtral-mini-latest", "parakeet-tdt-0.6b-v3"] {
            assert_eq!(kind_of(id), Some("stt"), "{id}");
        }
        for id in ["gpt-5-mini", "claude-sonnet-5-5", "llama-3.3-70b-versatile", "models/gemini-2.5-flash", "deepseek-chat", "mistral-small-latest", "qwen/qwen3-32b"] {
            assert_eq!(kind_of(id), Some("chat"), "{id}");
        }
        for id in ["dall-e-3", "gpt-image-1", "imagen-4.0-generate-001", "models/gemini-2.5-flash-image", "flux-1.1-pro", "stable-diffusion-xl", "sora-2", "veo-3.0-generate", "tts-1-hd", "gpt-4o-mini-tts", "canopylabs/orpheus-v1-english", "playai-tts", "text-embedding-3-small", "omni-moderation-latest", "llama-guard-4-12b", "models/gemini-2.5-flash-native-audio-latest", "gpt-realtime", "models/gemini-2.5-computer-use-preview-10-2025", "models/deep-research-max-preview-04-2026", "lyria-realtime-exp", "gpt-4o-audio-preview", "mistral-ocr-latest"] {
            assert_eq!(kind_of(id), None, "{id}");
        }
    }

    #[test]
    fn model_lists_speech_vs_text_junk_dropped_prices_read() {
        let l = parse_model_list(
            &json!({ "data": [
            { "id": "whisper-large-v3-turbo" }, { "id": "text-embedding-3-small" }, { "id": "llama-3.1-8b-instant" },
            { "id": "vendor/cheap", "name": "Cheap", "pricing": { "prompt": "0.0000001", "completion": "0.0000004" }, "architecture": { "output_modalities": ["text"] } },
            { "id": "vendor/pic", "architecture": { "output_modalities": ["image"] } } ] }),
            false,
        );
        let ids: Vec<(&str, &str)> =
            l.iter().map(|m| (m["id"].as_str().unwrap(), m["task"].as_str().unwrap())).collect();
        assert_eq!(
            ids,
            [("llama-3.1-8b-instant", "chat"), ("vendor/cheap", "chat"), ("whisper-large-v3-turbo", "stt")]
        );
        assert_eq!(l[1]["price"], json!({ "inPerM": 0.1, "outPerM": 0.4 }));
        assert_eq!(l[1]["tier"], "bad");
        assert_eq!(
            parse_model_list(&json!(["llama3.2"]), true)[0]["price"],
            json!({ "inPerM": 0, "outPerM": 0 })
        );
        let p = presets();
        for k in ["openrouter", "groq", "xai", "mistral", "opencode", "ollama"] {
            assert!(p.as_array().unwrap().iter().any(|x| x["key"] == k), "{k}");
        }
    }
}
