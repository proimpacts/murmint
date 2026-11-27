//! Updates come from the website: it says which version its installer is, and hands the installer to a signed-in app.
//! The engine checks and downloads; the app window starts the installer, which updates Murmint where it is.

use crate::account::SITE;
use crate::core;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(6 * 3600);

static FOUND: Mutex<Option<Value>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn valid_sha256(v: &Value) -> Option<&str> {
    v.as_str().filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

fn promote_verified_download(part: &Path, dest: &Path, expected: &Value, got: &str) -> Result<(), String> {
    let Some(want) = valid_sha256(expected) else {
        let _ = std::fs::remove_file(part);
        return Err("the update is missing a valid SHA-256 digest".into());
    };
    if !want.eq_ignore_ascii_case(got) {
        let _ = std::fs::remove_file(part);
        return Err("the download was damaged".into());
    }
    if let Err(e) = std::fs::rename(part, dest) {
        let _ = std::fs::remove_file(part);
        return Err(e.to_string());
    }
    Ok(())
}

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
    let Some(sha256) = valid_sha256(&info["sha256"]) else {
        *LAST_ERROR.lock().unwrap() = Some("Update metadata is missing a valid SHA-256 digest.".into());
        return None;
    };
    let found = json!({ "version": version, "size": info["size"], "sha256": sha256, "page": format!("{SITE}/download") });
    *FOUND.lock().unwrap() = Some(found.clone());
    core::push("update:available", "main", found.clone());
    Some(found)
}

/// For the Check now button: what was found, or why nothing was.
pub async fn check_now() -> Value {
    match check(true).await.or_else(available).filter(|u| valid_sha256(&u["sha256"]).is_some()) {
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
    if valid_sha256(&found["sha256"]).is_none() {
        return fail("the update is missing a valid SHA-256 digest".into());
    }
    let Some(token) = crate::account::token() else { return fail("sign in to Murmint first".into()) };
    let mut r = match crate::CLIENT.get(format!("{SITE}/api/update/installer")).bearer_auth(token).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => return fail(format!("download failed ({})", r.status().as_u16())),
        Err(e) => return fail(e.to_string()),
    };
    let version = found["version"].as_str().unwrap_or_default().to_string();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let part = std::env::temp_dir().join(format!(
        "Murmint Setup {version}.{}.{}.{seq}.part",
        std::process::id(),
        crate::store::now_ms()
    ));
    let file = std::env::temp_dir().join(format!("Murmint Setup {version}.exe"));
    // written to disk as it arrives, so the installer is never held in memory whole
    let saved = async {
        let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        while let Some(chunk) = r.chunk().await.map_err(|e| e.to_string())? {
            hash.update(&chunk);
            std::io::Write::write_all(&mut out, &chunk).map_err(|e| e.to_string())?;
        }
        std::io::Write::flush(&mut out).map_err(|e| e.to_string())?;
        Ok::<String, String>(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
    };
    let got = match saved.await {
        Ok(h) => h,
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            return fail(e);
        }
    };
    // only run an installer that matches what the website says it is
    if let Err(why) = promote_verified_download(&part, &file, &found["sha256"], &got) {
        return fail(why);
    }
    json!({ "ok": true, "file": file, "version": version })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

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

    #[test]
    fn digest_must_be_64_ascii_hex_chars() {
        let good = "a".repeat(64);
        let good_upper = "0123456789ABCDEF".repeat(4);
        assert_eq!(valid_sha256(&json!(good)), Some(good.as_str()));
        assert_eq!(valid_sha256(&json!(good_upper)), Some(good_upper.as_str()));
        assert!(valid_sha256(&Value::Null).is_none());
        assert!(valid_sha256(&json!(123)).is_none());
        assert!(valid_sha256(&json!("")).is_none());
        assert!(valid_sha256(&json!("a".repeat(63))).is_none());
        assert!(valid_sha256(&json!("a".repeat(65))).is_none());
        assert!(valid_sha256(&json!(format!("{}z", "a".repeat(63)))).is_none());
    }

    #[test]
    fn download_promotion_deletes_temp_and_blocks_missing_malformed_or_mismatched_digest() {
        let dir = std::env::temp_dir().join(format!("murmint-upd-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let payload = b"installer bytes";
        let got = sha256_hex(payload);

        for bad in [Value::Null, json!(""), json!("not-hex"), json!("0".repeat(64))] {
            let part = dir.join("test.part");
            let dest = dir.join("test.exe");
            let _ = std::fs::remove_file(&dest);
            std::fs::write(&part, payload).unwrap();
            assert!(promote_verified_download(&part, &dest, &bad, &got).is_err());
            assert!(!part.exists(), "temp file must be deleted on bad digest: {bad}");
            assert!(!dest.exists(), "installer must not be created on bad digest: {bad}");
        }

        let part = dir.join("good.part");
        let dest = dir.join("good.exe");
        let _ = std::fs::remove_file(&dest);
        std::fs::write(&part, payload).unwrap();
        promote_verified_download(&part, &dest, &json!(got.to_uppercase()), &got).unwrap();
        assert!(!part.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn download_rejects_missing_or_malformed_sha256_before_returning_executable() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for bad in [json!({ "version": "9.9.9", "page": "https://murmint.app/download" }),
                    json!({ "version": "9.9.9", "sha256": null, "page": "https://murmint.app/download" }),
                    json!({ "version": "9.9.9", "sha256": "abc", "page": "https://murmint.app/download" })] {
            *FOUND.lock().unwrap() = Some(bad);
            let res = download().await;
            assert_eq!(res["ok"], false);
            assert_eq!(res["page"], "https://murmint.app/download");
            assert!(res.get("file").is_none());
        }
        *FOUND.lock().unwrap() = None;
    }
}
