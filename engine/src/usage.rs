use serde_json::{json, Value};

pub fn estimate_tokens(text: &str) -> u64 {
    ((text.chars().count() as f64) / 4.0).ceil() as u64
}

pub fn cost_stt(model: &Value, audio_sec: f64) -> f64 {
    model["price"]["perMinute"].as_f64().unwrap_or(0.0) * audio_sec / 60.0
}
pub fn cost_chat(model: &Value, tin: f64, tout: f64) -> f64 {
    (model["price"]["inPerM"].as_f64().unwrap_or(0.0) * tin
        + model["price"]["outPerM"].as_f64().unwrap_or(0.0) * tout)
        / 1e6
}

pub fn day_key(ms: i64) -> String {
    let secs = ms.div_euclid(1000) + local_offset_secs();
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(windows)]
fn local_offset_secs() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    unsafe {
        let mut tz = TIME_ZONE_INFORMATION::default();
        let id = GetTimeZoneInformation(&mut tz);
        let bias = tz.Bias + if id == 2 { tz.DaylightBias } else { tz.StandardBias };
        -(bias as i64) * 60
    }
}
#[cfg(not(windows))]
fn local_offset_secs() -> i64 {
    0
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const TYPING_WPM: f64 = 40.0;

pub fn summarize(usage: &Value, now_ms: i64, days: i64) -> Value {
    let get = |d: &Value, k: &str| d[k].as_f64().unwrap_or(0.0);
    let empty = json!({});
    let day_map = usage["days"].as_object();
    let mut series = vec![];
    for i in (0..days).rev() {
        let k = day_key(now_ms - i * 86_400_000);
        let d = day_map.and_then(|m| m.get(&k)).unwrap_or(&empty);
        series.push(json!({ "day": k, "words": get(d, "words"), "dictations": get(d, "dictations"), "cost": get(d, "cost") }));
    }
    let month_prefix = day_key(now_ms)[..7].to_string();
    let (mut tot, mut month) = ([0.0f64; 5], [0.0f64; 4]);
    let mut by_model: serde_json::Map<String, Value> = serde_json::Map::new();
    if let Some(m) = day_map {
        for (k, d) in m {
            tot[0] += get(d, "dictations");
            tot[1] += get(d, "words");
            tot[2] += get(d, "audioSec");
            tot[3] += get(d, "cost");
            tot[4] += get(d, "requests");
            if k.starts_with(&month_prefix) {
                month[0] += get(d, "dictations");
                month[1] += get(d, "words");
                month[2] += get(d, "cost");
                month[3] += get(d, "requests");
                if let Some(bm) = d["byModel"].as_object() {
                    for (r, mm) in bm {
                        let t = by_model.entry(r.clone()).or_insert(
                            json!({ "requests": 0, "cost": 0, "audioSec": 0, "tokensIn": 0, "tokensOut": 0 }),
                        );
                        for f in ["requests", "cost", "audioSec", "tokensIn", "tokensOut"] {
                            t[f] = json!(t[f].as_f64().unwrap_or(0.0) + get(mm, f));
                        }
                    }
                }
            }
        }
    }
    let wpm = if tot[2] > 0.0 { tot[1] / (tot[2] / 60.0) } else { 0.0 };
    let saved = (tot[1] / TYPING_WPM - tot[2] / 60.0).max(0.0);
    json!({
        "series": series,
        "total": { "dictations": tot[0], "words": tot[1], "audioSec": tot[2], "cost": tot[3], "requests": tot[4] },
        "month": { "dictations": month[0], "words": month[1], "cost": month[2], "requests": month[3] },
        "byModel": by_model, "wpmSpoken": wpm, "savedMin": saved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_and_costs() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_000), (2024, 10, 4));
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        assert!((cost_stt(&json!({ "price": { "perMinute": 0.006 } }), 30.0) - 0.003).abs() < 1e-9);
        assert!(
            (cost_chat(&json!({ "price": { "inPerM": 1.0, "outPerM": 2.0 } }), 1_000_000.0, 500_000.0) - 2.0)
                .abs()
                < 1e-9
        );
        assert_eq!(day_key(1_700_000_000_000).len(), 10);
    }
    #[test]
    fn summary_counts_this_month_and_the_series() {
        let now = 1_700_000_000_000;
        let today = day_key(now);
        let usage = json!({ "days": { today.clone(): { "dictations": 2, "words": 100, "audioSec": 60, "cost": 0.5, "requests": 3, "byModel": { "p::m": { "requests": 3, "cost": 0.5, "audioSec": 60, "tokensIn": 10, "tokensOut": 4 } } } } });
        let s = summarize(&usage, now, 14);
        assert_eq!(s["series"].as_array().unwrap().len(), 14);
        assert_eq!(s["series"][13]["words"], 100.0);
        assert_eq!(s["month"]["cost"], 0.5);
        assert_eq!(s["byModel"]["p::m"]["tokensIn"], 10.0);
        assert!(s["savedMin"].as_f64().unwrap() > 1.0);
    }
}
