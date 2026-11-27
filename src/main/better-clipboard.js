'use strict';
// Better Clipboard: what you copy (never what Murmint dictates), plus emojis and GIFs, in an overlay that Win+V opens.
// While it is on, Windows' own clipboard history is switched off; the setting it had is kept and put back when
// Better Clipboard is turned off, and by the uninstaller.
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { app, BrowserWindow, clipboard, nativeImage, net, screen } = require('electron');

const MAX = 60; // unpinned items kept
const dir = () => path.join(app.getPath('userData'), 'data', 'clipboard');
const listFile = () => path.join(dir(), 'items.json');
const placeFile = () => path.join(dir(), 'place.json');
const GIF_MAX = 15 * 1024 * 1024;
const GIF_TIMEOUT_MS = 15000;
const KEY = 'HKCU\\Software\\Microsoft\\Clipboard';
const SAVED = 'HKCU\\Software\\Murmint';

let items = null, core = null, win = null, target = 0;
let quitting = false, enableSeq = 0;

function sweepOrphans() {
  try {
    const d = dir();
    if (!fs.existsSync(d)) return;
    const keep = new Set((items || []).map((x) => (x && x.file ? path.resolve(x.file) : null)).filter(Boolean));
    for (const name of fs.readdirSync(d)) {
      if (!/\.(gif|png)$/i.test(name)) continue;
      const full = path.resolve(path.join(d, name));
      if (!keep.has(full)) {
        try { fs.unlinkSync(full); } catch { }
      }
    }
  } catch { }
}

function load() {
  if (items) return items;
  try { items = JSON.parse(fs.readFileSync(listFile(), 'utf8')); } catch { items = []; }
  if (!Array.isArray(items)) items = [];
  sweepOrphans();
  return items;
}
function save() {
  try { fs.mkdirSync(dir(), { recursive: true }); fs.writeFileSync(listFile(), JSON.stringify(items)); } catch { }
  if (win && !win.isDestroyed()) win.webContents.send('clipboard:items', items);
}
const id = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 6);

function key(x) {
  if (!x) return '';
  if (x.kind === 'files') return 'files:' + (Array.isArray(x.files) && x.files.length ? x.files.join('|') : x.text || '');
  if (x.kind === 'image') return x.hash ? 'img:' + x.hash : (x.file || '');
  return x.text || x.url || x.source || x.file || '';
}

function add(item) {
  load();
  const k = key(item);
  const same = items.findIndex((x) => x.kind === item.kind && key(x) === k);
  if (same >= 0) {
    const existing = items.splice(same, 1)[0];
    let file = existing.file || item.file;
    if (item.file && existing.file && item.file !== existing.file) {
      if (fs.existsSync(existing.file)) {
        try { fs.unlinkSync(item.file); } catch { }
        file = existing.file;
      } else {
        file = item.file;
      }
    }
    item = { ...existing, ...item, id: existing.id, file, pinned: existing.pinned, at: Date.now() };
  }
  items.unshift(item);
  let unpinned = 0;
  items = items.filter((x) => {
    if (x.pinned) return true;
    if (++unpinned <= MAX) return true;
    if ((x.kind === 'image' || x.kind === 'gif') && x.file) { try { fs.unlinkSync(x.file); } catch { } }
    return false;
  });
  save();
}

// GIFs are kept as files, so they show animated and paste as the GIF itself (it plays in Discord, Slack, email)
async function keepGif(item, from) {
  const file = path.join(dir(), item.id + '.gif');
  try {
    fs.mkdirSync(dir(), { recursive: true });
    if (from.url) {
      const ac = new AbortController();
      const timer = setTimeout(() => ac.abort(), GIF_TIMEOUT_MS);
      if (timer.unref) timer.unref();
      try {
        const r = await net.fetch(from.url, { signal: ac.signal });
        if (!r.ok) { ac.abort(); return null; }
        const cl = Number(r.headers.get('content-length'));
        if (Number.isFinite(cl) && cl > GIF_MAX) { ac.abort(); return null; }
        if (!r.body || typeof r.body.getReader !== 'function') {
          const buf = Buffer.from(await r.arrayBuffer());
          if (buf.length > GIF_MAX || buf.slice(0, 3).toString() !== 'GIF') return null;
          fs.writeFileSync(file, buf);
        } else {
          const reader = r.body.getReader();
          const chunks = [];
          let total = 0;
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            const chunk = Buffer.from(value);
            total += chunk.length;
            if (total > GIF_MAX) {
              ac.abort();
              try { await reader.cancel(); } catch { }
              return null;
            }
            chunks.push(chunk);
          }
          const buf = Buffer.concat(chunks, total);
          if (buf.length > GIF_MAX || buf.slice(0, 3).toString() !== 'GIF') return null;
          fs.writeFileSync(file, buf);
        }
      } finally {
        clearTimeout(timer);
      }
    } else {
      if (fs.statSync(from.file).size > GIF_MAX) return null;
      fs.copyFileSync(from.file, file);
    }
  } catch { return null; }
  add({ ...item, file });
  return file;
}

// a copy the engine saw: { text, files, gif, image }
function copied(c) {
  const at = Date.now();
  if (c.gif) {
    load();
    const existing = items.find((x) => x.kind === 'gif' && (x.source === c.gif || x.url === c.gif) && x.file && fs.existsSync(x.file));
    if (existing) return add({ ...existing, source: existing.source || c.gif, at });
    return keepGif({ id: id(), kind: 'gif', url: c.gif, source: c.gif, at }, { url: c.gif });
  }
  const gifFile = (c.files || []).find((f) => /\.gif$/i.test(f));
  if (gifFile && c.files.length === 1) {
    load();
    const existing = items.find((x) => x.kind === 'gif' && x.source === gifFile && x.file && fs.existsSync(x.file));
    if (existing) return add({ ...existing, at });
    return keepGif({ id: id(), kind: 'gif', source: gifFile, at }, { file: gifFile });
  }
  if (c.image && !c.text) {
    const img = clipboard.readImage();
    if (img.isEmpty()) return;
    const png = img.resize({ width: Math.min(1600, img.getSize().width) }).toPNG();
    const hash = crypto.createHash('sha256').update(png).digest('hex').slice(0, 16);
    load();
    const existing = items.find((x) => x.kind === 'image' && x.hash === hash && x.file && fs.existsSync(x.file));
    if (existing) return add({ ...existing, at });
    const itemId = id();
    const file = path.join(dir(), itemId + '.png');
    try { fs.mkdirSync(dir(), { recursive: true }); fs.writeFileSync(file, png); } catch { return; }
    return add({ id: itemId, kind: 'image', hash, file, at });
  }
  if (c.files && c.files.length) return add({ id: id(), kind: 'files', files: c.files, text: c.files.join('\n'), at });
  if (c.text && c.text.length <= 20000) add({ id: id(), kind: 'text', text: c.text, at });
}

// ---- Windows' own clipboard history: off while Better Clipboard is on ----
const reg = (args) => new Promise((res) => execFile('reg.exe', args, { windowsHide: true }, (e, out) => res(e ? null : String(out))));
const readDword = async (keyName, name) => { const out = await reg(['query', keyName, '/v', name]); const m = out && out.match(/0x([0-9a-f]+)/i); return m ? parseInt(m[1], 16) : null; };
async function windowsHistory(off, seq) {
  if (process.platform !== 'win32') return;
  if (off && (quitting || (seq != null && seq !== enableSeq))) return;
  const saved = await readDword(SAVED, 'ClipboardHistoryWas');
  if (off && (quitting || (seq != null && seq !== enableSeq))) return;
  if (off) {
    if (saved == null) {
      const cur = (await readDword(KEY, 'EnableClipboardHistory')) ?? 0;
      if (quitting || (seq != null && seq !== enableSeq)) return;
      await reg(['add', SAVED, '/v', 'ClipboardHistoryWas', '/t', 'REG_DWORD', '/d', String(cur), '/f']);
    }
    if (quitting || (seq != null && seq !== enableSeq)) return;
    await reg(['add', KEY, '/v', 'EnableClipboardHistory', '/t', 'REG_DWORD', '/d', '0', '/f']);
  } else if (saved != null) {
    await reg(['add', KEY, '/v', 'EnableClipboardHistory', '/t', 'REG_DWORD', '/d', String(saved), '/f']);
    await reg(['delete', SAVED, '/v', 'ClipboardHistoryWas', '/f']);
  }
}

async function enable(engine, on) {
  core = engine;
  await core.ui('clipboard:enable', !!on).catch(() => {});
  await windowsHistory(!!on);
  if (!on && win && !win.isDestroyed()) win.hide();
}

// ---- the overlay ----
function overlay() {
  if (win && !win.isDestroyed()) return win;
  win = new BrowserWindow({
    width: 380, height: 470, show: false, frame: false, resizable: false, skipTaskbar: true, alwaysOnTop: true,
    transparent: true, hasShadow: false, fullscreenable: false, maximizable: false, minimizable: false,
    webPreferences: { preload: path.join(__dirname, '..', 'preload.js'), contextIsolation: true, sandbox: true, spellcheck: false },
  });
  win.setAlwaysOnTop(true, 'pop-up-menu');
  win.loadFile(path.join(__dirname, '..', 'renderer', 'clipboard', 'index.html'));
  win.on('blur', () => { if (!dragging && win && !win.isDestroyed() && win.isVisible()) win.hide(); });
  return win;
}

// Moved by hand rather than with -webkit-app-region, which a transparent always-on-top window ignores. Where it is
// dropped is where it opens next time.
let dragging = null;
function drag(on) {
  if (dragging) { clearInterval(dragging.timer); dragging = null; }
  if (!win || win.isDestroyed()) return;
  if (!on) {
    try { const [x, y] = win.getPosition(); fs.writeFileSync(placeFile(), JSON.stringify({ x, y })); } catch { }
    return;
  }
  const p = screen.getCursorScreenPoint();
  const [x, y] = win.getPosition();
  const [dx, dy] = [p.x - x, p.y - y];
  dragging = { timer: setInterval(() => {
    if (!win || win.isDestroyed()) return drag(false);
    const c = screen.getCursorScreenPoint();
    win.setPosition(c.x - dx, c.y - dy);
  }, 8) };
}

// Win+V (or Win+. for emoji): open beside the mouse, on its screen
function open(hwnd, tab) {
  target = hwnd || 0;
  const w = overlay();
  const [W, H] = w.getSize();
  let place = null;
  try { place = JSON.parse(fs.readFileSync(placeFile(), 'utf8')); } catch { }
  // where it was dragged to, if that is still on a screen; otherwise beside the mouse
  const fits = place && screen.getAllDisplays().some((d) => place.x >= d.workArea.x - W / 2 && place.x <= d.workArea.x + d.workArea.width - W / 2 && place.y >= d.workArea.y && place.y <= d.workArea.y + d.workArea.height - 40);
  if (fits) w.setPosition(place.x, place.y);
  else {
    const p = screen.getCursorScreenPoint();
    const wa = screen.getDisplayNearestPoint(p).workArea;
    w.setPosition(Math.round(Math.min(Math.max(p.x - W / 2, wa.x + 8), wa.x + wa.width - W - 8)), Math.round(Math.min(Math.max(p.y - H - 12, wa.y + 8), wa.y + wa.height - H - 8)));
  }
  const show = () => { w.webContents.send('clipboard:items', load()); w.webContents.send('clipboard:shown', tab === 'emoji' ? 'emoji' : 'clips'); w.show(); w.focus(); };
  if (w.webContents.isLoading()) w.webContents.once('did-finish-load', show); else show();
}

// a pick: an item id, or { emoji } / { url }
async function pick(what) {
  if (win && !win.isDestroyed()) win.hide();
  const item = typeof what === 'string' ? load().find((x) => x.id === what) : what;
  if (!item) return;
  if (item.kind === 'gif' && item.file && fs.existsSync(item.file)) {
    await core.ui('clipboard:paste_file', { hwnd: target, path: item.file });
    return;
  }
  if (item.kind === 'files') {
    const paths = Array.isArray(item.files) && item.files.length ? item.files : String(item.text || '').split(/\r?\n/).filter(Boolean);
    if (!paths.length) return;
    await core.ui('clipboard:paste_file', { hwnd: target, paths });
    return;
  }
  if (item.kind === 'image') {
    const img = nativeImage.createFromPath(item.file);
    if (img.isEmpty()) return;
    await core.ui('clipboard:suppress', 800);
    clipboard.writeImage(img);
    await core.ui('clipboard:paste_current', { hwnd: target });
    return;
  }
  await core.ui('clipboard:paste', { hwnd: target, text: item.emoji || item.url || item.text || '' });
}

function pin(itemId) { const x = load().find((i) => i.id === itemId); if (x) { x.pinned = !x.pinned; save(); } }
function remove(itemId) {
  const i = load().findIndex((x) => x.id === itemId);
  if (i < 0) return;
  const [x] = items.splice(i, 1);
  if (x.kind === 'image' || x.kind === 'gif') fs.rm(x.file, { force: true }, () => {});
  save();
}
function clear() {
  for (const x of load().filter((i) => !i.pinned && (i.kind === 'image' || i.kind === 'gif'))) fs.rm(x.file, { force: true }, () => {});
  items = load().filter((i) => i.pinned);
  save();
}
function close() { if (win && !win.isDestroyed()) win.hide(); }

module.exports = { enable, copied, open, pick, pin, remove, clear, close, drag, list: load };
