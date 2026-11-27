//! The Murmint account, required to use the app: sign in with Discord through murmint.app using a short code
//! (device-code flow). The sign-in is kept encrypted with the other secrets.

use crate::core;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const SITE: &str = "https://murmint.app";
const KEY: &str = "murmint-account"; // { token, user }

static ATTEMPT: AtomicU64 = AtomicU64::new(0);
static GEN: AtomicU64 = AtomicU64::new(0);

fn saved() -> Option<Value> {
    let raw = core::get().store.lock().unwrap().get_secret(KEY);
    serde_json::from_str::<Value>(&raw).ok().filter(|a| a["token"].is_string())
}
fn save(account: Option<&Value>) -> Result<(), String> {
    core::get()
        .store
        .lock()
        .unwrap()
        .try_set_secret(KEY, &account.map(|a| a.to_string()).unwrap_or_default())?;
    core::changed("account");
    Ok(())
}

pub fn user() -> Value {
    saved().map(|a| a["user"].clone()).unwrap_or(Value::Null)
}
pub fn signed_in() -> bool {
    saved().is_some()
}
/// The sign-in token, for downloads that need it (the update installer)
pub fn token() -> Option<String> {
    saved().and_then(|a| a["token"].as_str().map(String::from))
}

/// A sign-in carried over from an earlier version, which kept it in its own file.
pub fn import(account: &Value) -> Value {
    let Some(tok) = account["token"].as_str().filter(|t| !t.trim().is_empty()) else {
        return json!({ "ok": false, "error": "Invalid account token" });
    };
    let payload = json!({ "token": tok, "user": account["user"] }).to_string();
    let res = {
        let mut s = core::get().store.lock().unwrap();
        let existing = serde_json::from_str::<Value>(&s.get_secret(KEY))
            .ok()
            .and_then(|a| a["token"].as_str().map(|t| !t.trim().is_empty()))
            .unwrap_or(false);
        if existing {
            if s.is_degraded("secrets.json") {
                Err("Credential store is degraded".to_string())
            } else {
                Ok(false)
            }
        } else {
            s.try_set_secret(KEY, &payload).and_then(|()| {
                s.flush();
                if s.has_unsaved() || s.is_degraded("secrets.json") {
                    Err("Could not persist imported account".to_string())
                } else {
                    Ok(true)
                }
            })
        }
    };
    match res {
        Ok(did_save) => {
            GEN.fetch_add(1, Ordering::SeqCst);
            if did_save {
                core::changed("account");
            }
            json!({ "ok": true })
        }
        Err(e) => json!({ "ok": false, "error": e }),
    }
}

async fn post(path: &str, body: Value, token: Option<&str>) -> reqwest::Result<reqwest::Response> {
    let mut req = crate::CLIENT.post(format!("{SITE}{path}")).json(&body).timeout(Duration::from_secs(15));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    req.send().await
}

/// Starts a sign-in: the code goes to the app window (which also opens the page), then this waits for it to be used.
pub async fn sign_in() -> Value {
    let mine = ATTEMPT.fetch_add(1, Ordering::SeqCst) + 1;
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let start = match post("/api/link/start", json!({}), None).await {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.unwrap_or_default(),
        Ok(r) => return json!({ "ok": false, "error": format!("Could not reach murmint.app: murmint.app answered {}", r.status().as_u16()) }),
        Err(e) => return json!({ "ok": false, "error": format!("Could not reach murmint.app: {e}") }),
    };
    if ATTEMPT.load(Ordering::SeqCst) != mine || GEN.load(Ordering::SeqCst) != gen {
        return json!({ "ok": false, "cancelled": true });
    }
    core::push("account:code", "main", json!({ "userCode": start["userCode"], "verifyUrl": start["verifyUrl"] }));
    let deadline = Instant::now() + Duration::from_secs(start["expiresIn"].as_u64().unwrap_or(600));
    let every = Duration::from_secs(start["interval"].as_u64().unwrap_or(2).max(1));
    while Instant::now() < deadline {
        tokio::time::sleep(every).await;
        if ATTEMPT.load(Ordering::SeqCst) != mine || GEN.load(Ordering::SeqCst) != gen {
            return json!({ "ok": false, "cancelled": true });
        }
        let Ok(r) = post("/api/link/poll", json!({ "deviceCode": start["deviceCode"] }), None).await else {
            continue; // offline for a moment; keep trying
        };
        if ATTEMPT.load(Ordering::SeqCst) != mine || GEN.load(Ordering::SeqCst) != gen {
            return json!({ "ok": false, "cancelled": true });
        }
        if r.status().as_u16() == 202 {
            continue;
        }
        if !r.status().is_success() {
            break;
        }
        let Ok(j) = r.json::<Value>().await else { continue };
        let Some(tok) = j["token"].as_str().filter(|t| !t.trim().is_empty()) else {
            return json!({ "ok": false, "error": "Invalid sign-in response from murmint.app." });
        };
        let saved_res = {
            let mut s = core::get().store.lock().unwrap();
            if ATTEMPT.load(Ordering::SeqCst) != mine || GEN.load(Ordering::SeqCst) != gen {
                return json!({ "ok": false, "cancelled": true });
            }
            s.try_set_secret(KEY, &json!({ "token": tok, "user": j["user"] }).to_string())
        };
        return match saved_res {
            Ok(()) => {
                core::changed("account");
                json!({ "ok": true, "user": j["user"] })
            }
            Err(e) => json!({ "ok": false, "error": e }),
        };
    }
    json!({ "ok": false, "error": "The code expired. Try again." })
}

pub fn cancel() {
    ATTEMPT.fetch_add(1, Ordering::SeqCst);
    GEN.fetch_add(1, Ordering::SeqCst);
}

/// Asks murmint.app whether the saved sign-in still holds. Only an explicit "signed out" answer signs the app out,
/// so Murmint keeps working offline.
pub async fn verify() -> Value {
    let (gen, mut account, token) = {
        let s = core::get().store.lock().unwrap();
        let gen = GEN.load(Ordering::SeqCst);
        let raw = s.get_secret(KEY);
        let Some(account) = serde_json::from_str::<Value>(&raw).ok().filter(|a| a["token"].is_string()) else {
            return Value::Null;
        };
        let token = account["token"].as_str().unwrap_or_default().to_string();
        (gen, account, token)
    };
    let r = crate::CLIENT.get(format!("{SITE}/api/account")).bearer_auth(&token).timeout(Duration::from_secs(15)).send().await;
    match r {
        Ok(r) if r.status().as_u16() == 401 => {
            let cleared = {
                let mut s = core::get().store.lock().unwrap();
                let cur_tok = serde_json::from_str::<Value>(&s.get_secret(KEY))
                    .ok()
                    .and_then(|a| a["token"].as_str().map(String::from));
                if GEN.load(Ordering::SeqCst) == gen && cur_tok.as_deref() == Some(token.as_str()) {
                    s.try_set_secret(KEY, "").is_ok()
                } else {
                    false
                }
            };
            if cleared {
                core::changed("account");
            }
            return user();
        }
        Ok(r) if r.status().is_success() => {
            if let Ok(j) = r.json::<Value>().await {
                if let (Some(u), Some(fresh)) = (account["user"].as_object_mut(), j.as_object()) {
                    u.extend(fresh.clone());
                    let updated = {
                        let mut s = core::get().store.lock().unwrap();
                        let cur_tok = serde_json::from_str::<Value>(&s.get_secret(KEY))
                            .ok()
                            .and_then(|a| a["token"].as_str().map(String::from));
                        if GEN.load(Ordering::SeqCst) == gen && cur_tok.as_deref() == Some(token.as_str()) {
                            s.try_set_secret(KEY, &account.to_string()).is_ok()
                        } else {
                            false
                        }
                    };
                    if updated {
                        core::changed("account");
                    }
                }
            }
        }
        _ => {} // offline: keep the saved sign-in
    }
    user()
}

pub async fn sign_out() -> Value {
    ATTEMPT.fetch_add(1, Ordering::SeqCst);
    GEN.fetch_add(1, Ordering::SeqCst);
    let token = token();
    let _ = save(None);
    if let Some(t) = token {
        tokio::spawn(async move {
            let _ = post("/api/account/signout", json!({}), Some(&t)).await;
        });
    }
    json!(true)
}
