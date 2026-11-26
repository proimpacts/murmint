//! The Murmint account, required to use the app: sign in with Discord through murmint.app using a short code
//! (device-code flow). The sign-in is kept encrypted with the other secrets.

use crate::core;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const SITE: &str = "https://murmint.app";
const KEY: &str = "murmint-account"; // { token, user }

static ATTEMPT: AtomicU64 = AtomicU64::new(0);

fn saved() -> Option<Value> {
    let raw = core::get().store.lock().unwrap().get_secret(KEY);
    serde_json::from_str::<Value>(&raw).ok().filter(|a| a["token"].is_string())
}
fn save(account: Option<&Value>) {
    core::get().store.lock().unwrap().set_secret(KEY, &account.map(|a| a.to_string()).unwrap_or_default());
    core::changed("account");
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
    if account["token"].is_string() && saved().is_none() {
        save(Some(&json!({ "token": account["token"], "user": account["user"] })));
    }
    json!(true)
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
    let start = match post("/api/link/start", json!({}), None).await {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.unwrap_or_default(),
        Ok(r) => return json!({ "ok": false, "error": format!("Could not reach murmint.app: murmint.app answered {}", r.status().as_u16()) }),
        Err(e) => return json!({ "ok": false, "error": format!("Could not reach murmint.app: {e}") }),
    };
    core::push("account:code", "main", json!({ "userCode": start["userCode"], "verifyUrl": start["verifyUrl"] }));
    let deadline = Instant::now() + Duration::from_secs(start["expiresIn"].as_u64().unwrap_or(600));
    let every = Duration::from_secs(start["interval"].as_u64().unwrap_or(2).max(1));
    while Instant::now() < deadline {
        tokio::time::sleep(every).await;
        if ATTEMPT.load(Ordering::SeqCst) != mine {
            return json!({ "ok": false, "cancelled": true });
        }
        let Ok(r) = post("/api/link/poll", json!({ "deviceCode": start["deviceCode"] }), None).await else {
            continue; // offline for a moment; keep trying
        };
        if r.status().as_u16() == 202 {
            continue;
        }
        if !r.status().is_success() {
            break;
        }
        let Ok(j) = r.json::<Value>().await else { continue };
        save(Some(&json!({ "token": j["token"], "user": j["user"] })));
        return json!({ "ok": true, "user": j["user"] });
    }
    json!({ "ok": false, "error": "The code expired. Try again." })
}

pub fn cancel() {
    ATTEMPT.fetch_add(1, Ordering::SeqCst);
}

/// Asks murmint.app whether the saved sign-in still holds. Only an explicit "signed out" answer signs the app out,
/// so Murmint keeps working offline.
pub async fn verify() -> Value {
    let Some(mut account) = saved() else { return Value::Null };
    let token = account["token"].as_str().unwrap_or_default().to_string();
    let r = crate::CLIENT.get(format!("{SITE}/api/account")).bearer_auth(&token).timeout(Duration::from_secs(15)).send().await;
    match r {
        Ok(r) if r.status().as_u16() == 401 => {
            save(None);
            return Value::Null;
        }
        Ok(r) if r.status().is_success() => {
            if let (Ok(j), Some(u)) = (r.json::<Value>().await, account["user"].as_object_mut()) {
                if let Some(fresh) = j.as_object() {
                    u.extend(fresh.clone());
                }
                save(Some(&account));
            }
        }
        _ => {} // offline: keep the saved sign-in
    }
    user()
}

pub async fn sign_out() -> Value {
    let token = token();
    save(None);
    if let Some(t) = token {
        tokio::spawn(async move {
            let _ = post("/api/account/signout", json!({}), Some(&t)).await;
        });
    }
    json!(true)
}
