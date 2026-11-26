//! Updates come from the website: it says which version its installer is, and hands the installer to a signed-in app.
//! The engine checks and downloads; the app window starts the installer, which updates Murmint where it is.

use crate::account::SITE;
use crate::core;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(6 * 3600);

static FOUND: Mutex<Option<Value>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

// "1.2.3" or "v1.2" as numbers
fn parts(v: &str) -> Vec<u64> {
    v.trim_start_matches(['v', 'V']).split(['.', '+', '-']).map(|n| n.parse().unwrap_or(0)).collect()
}
fn newer(a: &str, b: &str) -> bool {
    let (x, y) = (parts(a), parts(b));
    for i in 0..3 {
        let (p, q) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}
// The website names versions the way people see them ("5.0"); the app's own number is 0.5.0
fn shown(v: &str) -> String {
    match v.strip_prefix("0.") {
        Some(rest) if rest.split('.').count() == 2 && rest.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) => rest.to_string(),
        _ => v.to_string(),
    }
}

fn enabled() -> bool {
    std::env::var("MURMINT_DEV").is_err() && core::get().store.lock().unwrap().config["general"]["autoUpdate"] != json!(false)
}

/// Looks for a new version; `force` checks even with automatic checks turned off.
pub async fn check(force: bool) -> Option<Value> {
    if !force && !enabled() {
        return None;
    }
    *LAST_ERROR.lock().unwrap() = None;
    let url = format!("{SITE}/api/update?_={}", crate::store::now_ms());
    let r = match crate::CLIENT.get(url).timeout(Duration::from_secs(20)).send().await {
        Ok(r) => r,
        Err(_) => {
            *LAST_ERROR.lock().unwrap() = Some("Could not reach murmint.app. Check your internet connection.".into());
            return None;
        }
    };
    if !r.status().is_success() {
        *LAST_ERROR.lock().unwrap() = Some(format!("The website answered with an error ({}).", r.status().as_u16()));
        return None;
    }
    let info: Value = r.json().await.unwrap_or_default();
    let version = info["version"].as_str().unwrap_or_default().to_string();
    let mine = core::get().version.lock().unwrap().clone();
    if version.is_empty() || !newer(&shown(&version), &shown(&mine)) {
        return None;
    }
    let found = json!({ "version": version, "size": info["size"], "sha256": info["sha256"], "page": format!("{SITE}/download") });
    *FOUND.lock().unwrap() = Some(found.clone());
    core::push("update:available", "main", found.clone());
    Some(found)
}

/// For the Check now button: what was found, or why nothing was.
pub async fn check_now() -> Value {
    match check(true).await.or_else(available) {
        Some(u) => u,
        None => json!({ "none": true, "error": *LAST_ERROR.lock().unwrap() }),
    }
}

pub fn available() -> Option<Value> {
    FOUND.lock().unwrap().clone()
}

/// Checks a little after the engine starts, then every six hours.
pub fn start() {
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            check(false).await;
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// Downloads the installer and checks it is the one the website described. The app window then runs it.
pub async fn download() -> Value {
    let Some(found) = available() else { return json!({ "ok": false, "message": "No update to install" }) };
    let page = found["page"].clone();
    let fail = |why: String| json!({ "ok": false, "page": page, "message": format!("Could not download the update, so its page was opened instead: {why}") });
    let Some(token) = crate::account::token() else { return fail("sign in to Murmint first".into()) };
    let mut r = match crate::CLIENT.get(format!("{SITE}/api/update/installer")).bearer_auth(token).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => return fail(format!("download failed ({})", r.status().as_u16())),
        Err(e) => return fail(e.to_string()),
    };
    let version = found["version"].as_str().unwrap_or_default().to_string();
    let file = std::env::temp_dir().join(format!("Murmint Setup {version}.exe"));
    // written to disk as it arrives, so the installer is never held in memory whole
    let saved = async {
        let mut out = std::fs::File::create(&file).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
            hash.update(&chunk);
            std::io::Write::write_all(&mut out, &chunk).map_err(|e| e.to_string())?;
        }
        Ok::<String, String>(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
    };
    let got = match saved.await {
        Ok(h) => h,
        Err(e) => {
            let _ = std::fs::remove_file(&file);
            return fail(e);
        }
    };
    // only run an installer that matches what the website says it is
    if found["sha256"].as_str().is_some_and(|want| want != got) {
        let _ = std::fs::remove_file(&file);
        return fail("the download was damaged".into());
    }
    json!({ "ok": true, "file": file, "version": version })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_the_way_people_see_them() {
        assert!(!newer(&shown("5.0"), &shown("0.5.0")));
        assert!(newer(&shown("5.1"), &shown("0.5.0")));
        assert!(newer(&shown("0.5.1"), &shown("0.5.0")));
        assert!(!newer(&shown("5.0"), &shown("0.5.1")));
        assert!(newer(&shown("5.0.1"), &shown("0.5.0")));
        assert_eq!(shown("0.5.0"), "5.0");
        assert_eq!(shown("1.2.3"), "1.2.3");
    }
}
