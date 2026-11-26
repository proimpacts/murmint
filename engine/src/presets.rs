use crate::re;
use serde_json::{json, Value};

pub fn presets() -> Value {
    serde_json::from_str(include_str!("../data/presets.json")).unwrap_or(json!([]))
}

fn is_stt(id: &str) -> bool {
    re!(r"(?i)whisper|transcri|voxtral|\bstt\b|speech-to-text|parakeet").is_match(id).unwrap_or(false)
}
fn skip(id: &str) -> bool {
    // not chat or stt
    re!(
        r"(?i)embed|\btts\b|text-to-speech|image|dall-?e|moderation|rerank|guard|realtime|sora|video|diffusion|flux|stable-|audio-preview|ocr"
    )
    .is_match(id)
    .unwrap_or(false)
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
        if id.is_empty() || skip(&id) {
            continue;
        }
        if let Some(mods) = r["architecture"]["output_modalities"].as_array() {
            if !mods.iter().any(|m| m == "text") {
                continue;
            }
        }
        let task = if is_stt(&id) { "stt" } else { "chat" };
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
