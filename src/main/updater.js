'use strict';
const { app, net, shell } = require('electron');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn } = require('child_process');

const REPO = 'proimpacts/murmint';
const EVERY = 6 * 3600e3;

const newer = (a, b) => {
  const p = (v) => String(v).replace(/^v/i, '').split(/[.+-]/).map((n) => parseInt(n, 10) || 0);
  const x = p(a), y = p(b);
  for (let i = 0; i < 3; i++) { if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0); }
  return false;
};

function create({ enabled, notify }) {
  let found = null, timer = null;

  let lastError = null;

  // lastError keeps a failed check from showing as "up to date"
  async function check(force = false) {
    if (!force && !enabled()) return null;
    lastError = null;
    try {
      const r = await net.fetch(`https://api.github.com/repos/${REPO}/releases/latest?_=${Date.now()}`, {
        headers: { 'User-Agent': 'Murmint-Updater', Accept: 'application/vnd.github+json' },
        cache: 'no-store',
      });
      if (!r.ok) { lastError = r.status === 403 || r.status === 429 ? 'GitHub is limiting requests right now. Try again in a few minutes.' : `GitHub answered with an error (${r.status}).`; return null; }
      const rel = await r.json();
      if (!rel || rel.draft || rel.prerelease || !rel.tag_name) return null;
      if (!newer(rel.tag_name, app.getVersion())) return null;
      const asset = (rel.assets || []).find((a) => /^Murmint[ .]Setup[ .].*.exe$/i.test(a.name)) || (rel.assets || []).find((a) => /\.exe$/i.test(a.name));
      if (!asset) { lastError = 'The newest release has no installer attached yet.'; return null; }
      found = { version: String(rel.tag_name).replace(/^v/i, ''), url: asset.browser_download_url, size: asset.size, page: rel.html_url };
      notify(found);
      return found;
    } catch { lastError = 'Could not reach GitHub. Check your internet connection.'; return null; }
  }

  async function install() {
    if (!found) return { ok: false, message: 'No update to install' };
    try {
      const r = await net.fetch(found.url, { headers: { 'User-Agent': 'Murmint-Updater' } });
      if (!r.ok) throw new Error('download failed (' + r.status + ')');
      const file = path.join(os.tmpdir(), `Murmint Setup ${found.version}.exe`);
      fs.writeFileSync(file, Buffer.from(await r.arrayBuffer()));
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

  return { start, check, install, get available() { return found; }, get error() { return lastError; } };
}

module.exports = { create, newer };
