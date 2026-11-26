'use strict';
const { app, BrowserWindow, ipcMain, session: electronSession, safeStorage, nativeTheme, clipboard, net } = require('electron');
const fs = require('fs');
const path = require('path');

const { Core } = require('./core');
const { Windows } = require('./windows');
const account = require('./account');
const localModels = require('./local-models');
const betterClipboard = require('./better-clipboard');
const translateView = require('./translate-view');

try { require('module').enableCompileCache && require('module').enableCompileCache(path.join(require('os').tmpdir(), 'murmint-js-cache')); } catch { }
if (process.env.MURMINT_LEAN !== '0') {
  app.disableHardwareAcceleration();
  app.commandLine.appendSwitch('in-process-gpu');
  app.commandLine.appendSwitch('disable-features', 'AudioServiceOutOfProcess,CalculateNativeWinOcclusion,MediaRouter,OptimizationHints,Translate,SpareRendererForSitePerProcess,BackForwardCache,WinRetrieveSuggestionsOnlyOnDemand,PrintCompositorLPAC,InterestFeedContentSuggestions,HardwareMediaKeyHandling,GlobalMediaControls');
  for (const sw of ['disable-renderer-accessibility', 'disable-extensions', 'disable-breakpad', 'disable-print-preview', 'disable-speech-api', 'disable-hang-monitor', 'disable-domain-reliability', 'disable-client-side-phishing-detection', 'disable-default-apps', 'metrics-recording-only', 'no-default-browser-check', 'disable-gpu-shader-disk-cache']) app.commandLine.appendSwitch(sw);
  app.commandLine.appendSwitch('disk-cache-size', '1');
  app.commandLine.appendSwitch('media-cache-size', '1');
  app.commandLine.appendSwitch('enable-features', 'NetworkServiceInProcess2');
  app.commandLine.appendSwitch('disable-background-networking');
  app.commandLine.appendSwitch('disable-component-update');
  app.commandLine.appendSwitch('disable-sync');
  app.commandLine.appendSwitch('no-pings');
  app.commandLine.appendSwitch('js-flags', '--max-lazy --optimize-for-size --max-old-space-size=64');
}
const DEV = !app.isPackaged;
const local = DEV ? (() => { try { return require('./local'); } catch { return null; } })() : null;
const isolated = local ? local.before(app) : false;
if (!isolated && !app.requestSingleInstanceLock()) { app.quit(); return; }
app.setAppUserModelId('dev.murmint.app');
if (DEV) process.env.MURMINT_DEV = '1';

let core, windows;
let updater = null;
let trimSoon = () => {};
let siteIcon = () => Promise.resolve(null);
const shell = { config: null, session: { state: 'idle', mode: null }, state: null };

const store = {
  get config() { return shell.config; },
  set(dotted, value) {
    const parts = dotted.split('.'); let o = shell.config;
    for (let i = 0; i < parts.length - 1; i++) o = o[parts[i]] ??= {};
    o[parts[parts.length - 1]] = value;
    core.ui('config:set', dotted, value).catch(() => {});
  },
};

function migrateFromSayso(dest) {
  try {
    if (fs.existsSync(path.join(dest, 'config.json'))) return;
    for (const name of ['Sayso', 'sayso']) {
      const src = path.join(app.getPath('appData'), name, 'data');
      if (!fs.existsSync(path.join(src, 'config.json'))) continue;
      fs.mkdirSync(dest, { recursive: true });
      for (const f of ['config.json', 'history.json', 'usage.json']) if (fs.existsSync(path.join(src, f))) fs.copyFileSync(path.join(src, f), path.join(dest, f));
      return;
    }
  } catch (e) { console.error('migration skipped:', e.message); }
}

async function migrateSecrets() {
  if (!safeStorage.isEncryptionAvailable()) return;
  try {
    const pending = await core.ui('secrets:pending');
    for (const { id, v } of pending || []) {
      try { await core.ui('secrets:import', id, safeStorage.decryptString(Buffer.from(v, 'base64'))); } catch { }
    }
  } catch (e) { console.error('key migration skipped:', e.message); }
}

function applySideEffects() {
  const cfg = shell.config;
  if (!cfg || !windows) return;
  if (windows.overlay && !windows.drag && !windows._snapping) { windows.positionOverlay(); windows.syncOverlay(shell.session.state !== 'idle'); }
  nativeTheme.themeSource = cfg.appearance.theme === 'system' ? 'system' : cfg.appearance.theme;
  windows.applyTheme();
  try { app.setLoginItemSettings({ openAtLogin: !!cfg.general.launchAtLogin, args: ['--hidden'] }); } catch { }
}

function wireCore() {
  core.on('push', ({ ch, to, payload }) => {
    if (ch === 'state:changed') { shell.state = payload; shell.config = payload.config; shell.session = payload.session || shell.session; }
    if (ch === 'shell:apply') { applySideEffects(); return; }
    if (ch === 'site:seen') { siteIcon(payload && payload.site).catch(() => {}); return; }
    if (ch === 'gate:needed') { windows && windows.createMain(); return; }
    if (ch === 'translate:show') { translateView.show(payload || {}).catch(() => {}); return; }
    if (ch === 'clipboard:copied') { betterClipboard.copied(payload); return; }
    if (ch === 'clipboard:open') { if (isOpen()) betterClipboard.open(payload && payload.hwnd, payload && payload.tab); return; }
    if (ch === 'overlay:state') {
      shell.session = { state: payload.state, mode: payload.mode };
      if (payload.state === 'idle') trimSoon(6000);
      if (payload.state === 'idle') setTimeout(() => { if (shell.session.state === 'idle') windows.syncOverlay(false); }, 220);
      else windows.syncOverlay(true);
    }
    if (!windows) return;
    if (to === 'all') windows.broadcast(ch, payload);
    else if (to === 'main') windows.toMain(ch, payload);
    else if (to === 'overlay') windows.toOverlay(ch, payload);
  });
  core.on('down', () => windows && windows.toMain('toast', { kind: 'error', message: 'The Murmint engine stopped. Restarting it…' }));
}

const b64 = (b) => Buffer.from(b).toString('base64');

async function memoryUse() {
  const metrics = app.getAppMetrics();
  let electronMB = 0, engineMB = 0;
  try {
    const r = await core.ui('app:memory', metrics.map((p) => p.pid));
    electronMB = (r.pids || []).reduce((n, x) => n + x, 0); engineMB = r.engine || 0;
  } catch { }
  return { total: Math.round(electronMB + engineMB), app: Math.round(electronMB), engine: Math.round(engineMB), processes: metrics.length + 1 };
}

function wireIpc() {
  for (const ch of ['state:get', 'config:set', 'provider:save', 'provider:remove', 'provider:key', 'provider:test', 'provider:presets', 'provider:models', 'provider:list', 'keys:capture', 'keys:down',
    'history:clear', 'history:delete', 'clipboard:write', 'usage:summary', 'usage:reset', 'router:preview', 'format:try',
    'session:action', 'session:undo']) {
    ipcMain.handle(ch, (_e, ...a) => core.ui(ch, ...a));
  }
  const installedApps = require('./installed-apps');
  ipcMain.handle('apps:classic', () => installedApps.classic());
  ipcMain.handle('apps:store', () => installedApps.store());
  ipcMain.handle('apps:pick', () => installedApps.pick(windows && windows.main));
  ipcMain.handle('account:get', () => account.user());
  ipcMain.handle('account:signIn', () => account.signIn((code) => windows && windows.toMain('account:code', code)));
  ipcMain.handle('account:cancel', () => account.cancel());
  ipcMain.handle('account:signOut', async () => { await account.signOut(); await gate(); });
  const localStatus = async () => ({ ...localModels.status(), free: await localModels.freeSpace() });
  ipcMain.handle('onboarding:get', async () => ({ user: account.user(), open: isOpen(), local: await localStatus() }));
  ipcMain.handle('onboarding:finish', async () => {
    store.set('onboarded', true);
    await useDefaults();
    await gate();
    return isOpen();
  });
  ipcMain.handle('local:status', () => localStatus());
  ipcMain.handle('clipboard:list', () => betterClipboard.list());
  ipcMain.handle('clipboard:pick', (_e, what) => betterClipboard.pick(what));
  ipcMain.handle('clipboard:pin', (_e, id) => betterClipboard.pin(id));
  ipcMain.handle('clipboard:remove', (_e, id) => betterClipboard.remove(id));
  ipcMain.handle('clipboard:clear', () => betterClipboard.clear());
  ipcMain.handle('clipboard:close', () => betterClipboard.close());
  ipcMain.handle('clipboard:drag', (_e, on) => betterClipboard.drag(!!on));
  ipcMain.handle('clipboard:enable', (_e, on) => { store.set('clipboard.enabled', !!on); return betterClipboard.enable(core, !!on && isOpen()); });
  ipcMain.handle('local:download', () => localModels.download(() => { if (isOpen()) localModels.use(core).catch(() => {}); }));
  ipcMain.handle('local:use', async () => ({ ok: await localModels.use(core) }));
  ipcMain.handle('local:cancel', () => localModels.cancel());
  ipcMain.handle('local:start', async () => ({ ok: await localModels.start(core) }));
  localModels.watch((p) => windows && windows.toMain('local:progress', p));
  ipcMain.handle('vocab:train', (_e, t) => core.ui('vocab:train', { wav_b64: b64(t.wav), durationSec: t.durationSec, term: t.term }));
  ipcMain.handle('rec:preview', (_e, p) => core.ui('rec:preview', { wav_b64: b64(p.wav), durationSec: p.durationSec }));
  ipcMain.on('rec:segment', (_e, seg) => core.send('rec:segment', { wav_b64: b64(seg.wav), durationSec: seg.durationSec, peak: seg.peak, final: !!seg.final, gapMs: seg.gapMs ?? null }));
  ipcMain.on('rec:chunk', (_e, buf) => core.send('rec:chunk', b64(buf)));
  ipcMain.on('rec:boundary', (_e, b) => core.send('rec:boundary', b));
  ipcMain.on('rec:autostop', () => core.send('rec:autostop'));
  ipcMain.on('rec:error', (_e, message) => core.send('rec:error', message));
  ipcMain.handle('win:action', (e, a) => {
    const w = BrowserWindow.fromWebContents(e.sender);
    if (!w) return;
    if (a === 'min') w.minimize(); else if (a === 'max') (w.isMaximized() ? w.unmaximize() : w.maximize()); else if (a === 'close') w.close();
  });
  ipcMain.on('overlay:hover', (_e, hovering) => {
    if (windows.drag) return;
    windows.overlay?.setIgnoreMouseEvents(!hovering, { forward: true });
  });
  ipcMain.on('overlay:drag', (_e, phase, p) => windows.dragOverlay(phase, p || {}));
  const icons = new Map();
  ipcMain.handle('app:icon', async (_e, exe) => {
    if (typeof exe !== 'string' || !/\.exe$/i.test(exe) || !fs.existsSync(exe)) return null;
    if (!icons.has(exe)) { try { icons.set(exe, (await app.getFileIcon(exe, { size: 'normal' })).toDataURL()); } catch { icons.set(exe, null); } }
    return icons.get(exe);
  });
  const favs = new Map();
  const fetchIcon = async (url) => {
    const ctl = new AbortController(); const t = setTimeout(() => ctl.abort(), 5000);
    try {
      const r = await net.fetch(url, { signal: ctl.signal, redirect: 'follow' });
      const type = (r.headers.get('content-type') || '').split(';')[0].trim();
      if (!r.ok) return null;
      const buf = Buffer.from(await r.arrayBuffer());
      if (!buf.length || buf.length > 300000) return null;
      if (type.startsWith('text/html')) return { html: buf.toString('utf8'), base: r.url };
      return 'data:' + (type.startsWith('image/') ? type : 'image/x-icon') + ';base64,' + buf.toString('base64');
    } catch { return null; } finally { clearTimeout(t); }
  };
  const pending = new Map();
  siteIcon = (host) => {
    if (typeof host !== 'string' || !/^[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]{2,}$/i.test(host)) return Promise.resolve(null);
    const known = favs.get(host);
    if (known && (known.icon || Date.now() - known.at < 60e3)) return Promise.resolve(known.icon);
    if (!pending.has(host)) pending.set(host, loadSiteIcon(host).finally(() => pending.delete(host)));
    return pending.get(host);
  };
  ipcMain.handle('site:icon', (_e, host) => siteIcon(host));
  async function loadSiteIcon(host) {
    let icon = await fetchIcon('https://' + host + '/favicon.ico');
    if (!icon || typeof icon !== 'string') {
      const page = await fetchIcon('https://' + host + '/');
      const html = page && page.html;
      const href = html && ((/<link[^>]+rel=["'][^"']*icon[^"']*["'][^>]*>/i.exec(html) || [])[0] || '').match(/href=["']([^"']+)["']/i);
      if (href) { const got = await fetchIcon(new URL(href[1], page.base).href); icon = typeof got === 'string' ? got : null; } else icon = null;
    }
    favs.set(host, { icon, at: Date.now() });
    return icon;
  }
  ipcMain.on('overlay:pin', (_e, on) => { windows.pinned = !!on; windows.syncOverlay(shell.session.state !== 'idle'); });
}

app.on('second-instance', () => windows?.createMain());

// Murmint needs a signed-in account and a finished onboarding before it records anything.
const isOpen = () => !!account.user() && shell.config?.onboarded === true;
async function gate() {
  const open = isOpen();
  await core.lock(!open).catch(() => {});
  if (open && !windows.overlay) { windows.createOverlay(); applySideEffects(); }
  betterClipboard.enable(core, open && shell.config?.clipboard?.enabled !== false).catch(() => {});
}

// What onboarding leaves set up: Murmint 1 for speech, and formatting on Automatic, which picks the best and cheapest
// model among the providers the person added.
async function useDefaults() {
  if (localModels.installed()) await localModels.use(core).catch(() => {});
  store.set('models.format.routing', 'auto');
}

function startLocalModels() {
  const cfg = shell.config;
  if (!localModels.installed()) return;
  if (!cfg.providers.some((p) => p.id === localModels.PROVIDER)) return; // removed under Models: leave it off
  localModels.start(core).catch((e) => console.error('local models:', e.message));
}

app.whenReady().then(async () => {
  core = new Core({ log: (l) => console.log('[core]', l) });
  if (!core.available) { console.error('The Murmint engine (murmint-engine.exe) is missing.'); app.quit(); return; }
  core.start();
  const dataDir = path.join(app.getPath('userData'), 'data');
  try { fs.rmSync(path.join(dataDir, 'extensions'), { recursive: true, force: true }); } catch { /* nothing to clean */ }
  if (!isolated) migrateFromSayso(dataDir);
  wireCore();
  await core.init(dataDir, app.getVersion(), true);
  shell.config = shell.config || (await core.ui('state:get')).config;
  windows = new Windows(store);
  windows.onRaise = (hwnd) => core.ui('overlay:raise', hwnd).catch(() => {});
  migrateSecrets();

  const ses = electronSession.defaultSession;
  ses.setPermissionRequestHandler((wc, permission, cb) => cb(permission === 'media' || permission === 'clipboard-sanitized-write'));
  ses.setPermissionCheckHandler((wc, permission) => permission === 'media');

  wireIpc();
  await gate();
  const hidden = process.argv.includes('--hidden') && isOpen();
  if (!hidden || DEV) windows.createMain({ show: !hidden });
  startLocalModels();
  const signedIn = !!account.user();
  account.verify().then((u) => { if (signedIn && !u) gate().then(() => windows.toMain('gate:changed')); }).catch(() => {});
  let trimTimer = null;
  const trim = async () => {
    try {
      if (shell.session.state !== 'idle') return;
      const open = windows.main && !windows.main.isDestroyed() && windows.main.isVisible() && windows.main.isFocused();
      // the main process and the pill (which records) stay in memory, so the shortcut responds at once
      const keep = new Set([process.pid, windows.overlay && !windows.overlay.isDestroyed() ? windows.overlay.webContents.getOSProcessId() : 0]);
      const pids = open ? [] : app.getAppMetrics().map((p) => p.pid).filter((pid) => !keep.has(pid));
      await core.ui('app:trim', pids);
      tip();
    } catch { }
  };
  trimSoon = (ms = 4000) => { clearTimeout(trimTimer); trimTimer = setTimeout(trim, ms); };
  windows.afterClose = () => trimSoon(1500);
  trimSoon(7000); setInterval(() => trimSoon(0), 300000).unref?.();
  const tip = async () => { try { const m = await memoryUse(); if (windows.tray) windows.tray.setToolTip(`Murmint · ${m.total} MB`); } catch { } };
  setTimeout(tip, 8000); setInterval(tip, 120000).unref?.();
  windows.createTray({ toggle: () => core.ui('session:action', 'toggle'), quit: () => app.quit() });
  applySideEffects();

  updater = require('./updater').create({
    enabled: () => !DEV && !(shell.config && shell.config.general && shell.config.general.autoUpdate === false),
    notify: (u) => windows && windows.toMain('update:available', u),
    token: () => require('./account').token(),
  });
  updater.start();
  ipcMain.handle('update:install', () => updater.install());
  ipcMain.handle('update:check', async () => { const u = (await updater.check(true)) || updater.available || null; return u ? u : { none: true, error: updater.error }; });
  ipcMain.handle('update:get', () => updater.available || null);

  local?.ready({ app, core, windows, store, shell, clipboard });
});

app.on('before-quit', () => { if (windows) windows.quitting = true; localModels.stop(); core?.stop(); });
app.on('window-all-closed', () => { });
