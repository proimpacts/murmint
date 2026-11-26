'use strict';
const { app, net, shell } = require('electron');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');

// Updates come from the website: it says which version its installer is, and hands the installer to a signed-in app.
const SITE = 'https://murmint.app';
const EVERY = 6 * 3600e3;

const newer = (a, b) => {
  const p = (v) => String(v).replace(/^v/i, '').split(/[.+-]/).map((n) => parseInt(n, 10) || 0);
  const x = p(a), y = p(b);
  for (let i = 0; i < 3; i++) { if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0); }
  return false;
};

function create({ enabled, notify, token }) {
  let found = null, timer = null;

  let lastError = null;
  let releaseAt = null; // when Murmint is released to everyone, from the website

  // lastError keeps a failed check from showing as "up to date"
  async function check(force = false) {
    if (!force && !enabled()) return null;
    lastError = null;
    try {
      const r = await net.fetch(`${SITE}/api/update?_=${Date.now()}`, { headers: { 'User-Agent': 'Murmint-Updater' }, cache: 'no-store' });
      if (!r.ok) { lastError = `The website answered with an error (${r.status}).`; return null; }
      const info = await r.json();
      releaseAt = info.releaseAt || releaseAt;
      if (!info.version || !newer(info.version, app.getVersion())) return null;
      found = { version: String(info.version), url: `${SITE}/api/update/installer`, size: info.size, sha256: info.sha256, page: `${SITE}/download` };
      notify(found);
      return found;
    } catch { lastError = 'Could not reach murmint.app. Check your internet connection.'; return null; }
  }

  async function install() {
    if (!found) return { ok: false, message: 'No update to install' };
    try {
      const t = token && token();
      if (!t) throw new Error('sign in to Murmint first');
      const r = await net.fetch(found.url, { headers: { 'User-Agent': 'Murmint-Updater', Authorization: `Bearer ${t}` }, cache: 'no-store' });
      if (!r.ok) throw new Error('download failed (' + r.status + ')');
      const file = path.join(os.tmpdir(), `Murmint Setup ${found.version}.exe`);
      const bytes = Buffer.from(await r.arrayBuffer());
      // only run an installer that matches what the website says it is
      if (found.sha256 && require('crypto').createHash('sha256').update(bytes).digest('hex') !== found.sha256) throw new Error('the download was damaged');
      fs.writeFileSync(file, bytes);
      spawn(file, [], { detached: true, stdio: 'ignore' }).unref();
      setTimeout(() => app.exit(0), 400);
      return { ok: true };
    } catch (e) {
      if (found.page) shell.openExternal(found.page);
      return { ok: false, message: 'Could not download the update, so its page was opened instead: ' + e.message };
    }
  }

  function start() {
    clearInterval(timer);
    setTimeout(check, 15e3);
    timer = setInterval(check, EVERY);
  }

  return { start, check, install, get available() { return found; }, get error() { return lastError; }, get releaseAt() { return releaseAt; } };
}

module.exports = { create, newer };
