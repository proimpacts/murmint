'use strict';
// Murmint account, required to use the app: sign in with Discord through murmint.app using a short code (device-code flow).
const fs = require('fs');
const path = require('path');
const { app, net, shell, safeStorage } = require('electron');

const SITE = 'https://murmint.app';
const file = () => path.join(app.getPath('userData'), 'data', 'account.bin');
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

let account; // { token, user }, null when signed out, undefined until loaded
let attempt = 0;

function load() {
  try {
    const raw = fs.readFileSync(file());
    account = JSON.parse(safeStorage.decryptString(raw));
  } catch { account = null; }
}

function save() {
  try {
    if (!account) return fs.rmSync(file(), { force: true });
    fs.writeFileSync(file(), safeStorage.encryptString(JSON.stringify(account)));
  } catch { /* keep it in memory for this run */ }
}

const post = (p, body, token) => net.fetch(SITE + p, {
  method: 'POST', cache: 'no-store',
  headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
  body: JSON.stringify(body || {}),
});

function user() {
  if (account === undefined) load();
  return account ? account.user : null;
}

// showCode({ userCode, verifyUrl }) is called once the code is ready
async function signIn(showCode) {
  const mine = ++attempt;
  let start;
  try {
    const r = await post('/api/link/start');
    if (!r.ok) throw new Error(`murmint.app answered ${r.status}`);
    start = await r.json();
  } catch (e) {
    return { ok: false, error: 'Could not reach murmint.app: ' + e.message };
  }
  showCode({ userCode: start.userCode, verifyUrl: start.verifyUrl });
  shell.openExternal(start.verifyUrl).catch(() => {});

  const deadline = Date.now() + start.expiresIn * 1000;
  while (Date.now() < deadline) {
    await wait((start.interval || 2) * 1000);
    if (mine !== attempt) return { ok: false, cancelled: true };
    try {
      const r = await post('/api/link/poll', { deviceCode: start.deviceCode });
      if (r.status === 202) continue;
      if (!r.ok) return { ok: false, error: 'The code expired. Try again.' };
      const j = await r.json();
      account = { token: j.token, user: j.user };
      save();
      return { ok: true, user: j.user };
    } catch { /* offline for a moment; keep trying */ }
  }
  return { ok: false, error: 'The code expired. Try again.' };
}

function cancel() { attempt++; }

// Asks murmint.app whether the saved sign-in still holds. Only an explicit "signed out" answer signs the app out,
// so Murmint keeps working offline.
async function verify() {
  if (account === undefined) load();
  if (!account) return null;
  try {
    const r = await net.fetch(SITE + '/api/account', { cache: 'no-store', headers: { Authorization: `Bearer ${account.token}` } });
    if (r.status === 401) { account = null; save(); return null; }
    if (r.ok) {
      const j = await r.json();
      account.user = { ...account.user, ...j };
      save();
    }
  } catch { /* offline: keep the saved sign-in */ }
  return account ? account.user : null;
}

async function signOut() {
  if (account === undefined) load();
  const token = account && account.token;
  account = null;
  save();
  if (token) post('/api/account/signout', null, token).catch(() => {});
}

module.exports = { user, signIn, cancel, signOut, verify };
