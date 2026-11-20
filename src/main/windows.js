'use strict';
const { BrowserWindow, screen, Tray, Menu, nativeImage, shell } = require('electron');
const path = require('path');

const ROOT = path.join(__dirname, '..', '..');
const R = (...p) => path.join(ROOT, 'src', 'renderer', ...p);
const BUILD = (f) => path.join(ROOT, 'build', f);

// Windows 11 (build 22000 and later) draws a Mica backdrop behind the main window
const MICA = process.platform === 'win32' && Number(require('os').release().split('.')[2]) >= 22000;

const THEME = {
  dark: { bg: '#000000', fg: '#a6a6ab' },
  light: { bg: '#f2f2f3', fg: '#55555b' },
};

class Windows {
  constructor(store) {
    this.store = store;
    this.main = null;
    this.overlay = null;
    this.tray = null;
    this.quitting = false;
  }

  resolvedTheme() {
    const t = this.store.config.appearance.theme;
    if (t === 'system') return require('electron').nativeTheme.shouldUseDarkColors ? 'dark' : 'light';
    return t;
  }

  createMain({ show = true } = {}) {
    if (this.main && !this.main.isDestroyed()) { this.main.show(); this.main.focus(); return this.main; }
    const th = THEME[this.resolvedTheme()];
    this.main = new BrowserWindow({
      width: 1120, height: 760, minWidth: 880, minHeight: 600,
      show: false, backgroundColor: MICA ? '#00000000' : th.bg, title: 'Murmint',
      ...(MICA ? { backgroundMaterial: 'mica' } : {}),
      icon: BUILD('icon.png'),
      titleBarStyle: 'hidden',
      titleBarOverlay: { color: MICA ? '#00000000' : th.bg, symbolColor: th.fg, height: 40 },
      webPreferences: { preload: path.join(__dirname, '..', 'preload.js'), contextIsolation: true, sandbox: true, spellcheck: false },
    });
    this.main.setMenuBarVisibility(false);
    this.main.loadFile(R('app', 'index.html'), MICA ? { query: { mica: '1' } } : undefined);
    this.main.once('ready-to-show', () => { if (show) this.main.show(); });
    this.main.webContents.setWindowOpenHandler(({ url }) => { if (/^https:/.test(url)) shell.openExternal(url); return { action: 'deny' }; });
    this.main.on('closed', () => { this.main = null; if (this.afterClose) this.afterClose(); });
    return this.main;
  }

  applyTheme() {
    if (!this.main || this.main.isDestroyed()) return;
    const th = THEME[this.resolvedTheme()];
    if (!MICA) this.main.setBackgroundColor(th.bg);
    try { this.main.setTitleBarOverlay({ color: MICA ? '#00000000' : th.bg, symbolColor: th.fg, height: 40 }); } catch { }
  }

  createOverlay() {
    this.baseW = 380; this.baseH = 340; this.scale = 1;
    this.OW = this.baseW; this.OH = this.baseH;
    this.overlay = new BrowserWindow({
      width: this.OW, height: this.OH, show: false, frame: false, transparent: true, resizable: false, movable: false,
      skipTaskbar: true, alwaysOnTop: true, focusable: false, hasShadow: false, fullscreenable: false,
      webPreferences: { preload: path.join(__dirname, '..', 'preload.js'), contextIsolation: true, sandbox: true, backgroundThrottling: false },
    });
    this._raiseOverlay();
    this.overlay.setIgnoreMouseEvents(true, { forward: true });
    this.overlay.loadFile(R('overlay', 'index.html'));
    this.overlayShown = false;
    this.overlay.webContents.once('did-finish-load', () => { this.positionOverlay(); this.syncOverlay(); });
    this.overlay.on('hide', () => { setTimeout(() => { const o = this.overlay; if (this.overlayShown && o && !o.isDestroyed() && !o.isVisible()) o.showInactive(); }, 250); });
    this.overlay.webContents.on('render-process-gone', (_e, d) => { console.log('[overlay] page gone:', d && d.reason); this._rebuildOverlay(); });
    this.overlay.on('unresponsive', () => { console.log('[overlay] unresponsive, reloading'); try { this.overlay.webContents.reload(); } catch { this._rebuildOverlay(); } });
    if (!this._watching) {
      this._watching = true;
      const refit = () => { if (!this.drag && !this._snapping) this.positionOverlay(); this._repaintOverlay(); };
      for (const ev of ['display-metrics-changed', 'display-added', 'display-removed']) screen.on(ev, refit);
      const { powerMonitor } = require('electron');
      for (const ev of ['resume', 'unlock-screen', 'user-did-become-active']) powerMonitor.on(ev, () => setTimeout(() => { refit(); }, 800));
      setInterval(() => this._repaintOverlay(), 20000).unref?.();
      setInterval(() => { const o = this.overlay; if (this.overlayShown && o && !o.isDestroyed()) { if (!o.isVisible()) o.showInactive(); this._raiseOverlay(); } }, 2000).unref?.();
    }
    return this.overlay;
  }

  _repaintOverlay() {
    const o = this.overlay;
    if (!o || o.isDestroyed()) { this._rebuildOverlay(); return; }
    if (o.webContents.isCrashed()) { this._rebuildOverlay(); return; }
    if (this.overlayShown) {
      if (!o.isVisible()) o.showInactive();
      // redraw only; raising here would cover the Start menu
      o.webContents.invalidate();
    }
  }

  _rebuildOverlay() {
    if (this._rebuilding) return;
    this._rebuilding = true;
    const old = this.overlay;
    this.overlay = null; this.overlayShown = false;
    try { if (old && !old.isDestroyed()) old.destroy(); } catch { }
    setTimeout(() => { this._rebuilding = false; this.createOverlay(); }, 600);
  }

  _pillDisplay() {
    if (this.overlay && this.overlayShown) {
      const b = this.overlay.getBounds();
      return screen.getDisplayNearestPoint({ x: b.x + Math.round(b.width / 2), y: b.y + Math.round(b.height / 2) });
    }
    return screen.getPrimaryDisplay();
  }

  _fitScale(display) {
    const s = Math.min(1.4, Math.max(0.9, display.size.height / 1080));
    this.scale = Math.round(s * 100) / 100;
    this.OW = Math.round(this.baseW * this.scale); this.OH = Math.round(this.baseH * this.scale);
    try { this.overlay.webContents.setZoomFactor(this.scale); } catch { }
  }

  _targetBounds(display, side, frac) {
    const wa = display.workArea;
    const f = Math.min(1, Math.max(0, frac));
    if (side === 'top' || side === 'bottom') {
      const x = wa.x + Math.round(f * Math.max(0, wa.width - this.OW));
      const y = side === 'top' ? wa.y : wa.y + wa.height - this.OH;
      return { x, y, width: this.OW, height: this.OH };
    }
    const x = side === 'left' ? wa.x : wa.x + wa.width - this.OW; // inside the work area, so never over a side taskbar
    const y = wa.y + Math.round(f * Math.max(0, wa.height - this.OH));
    return { x, y, width: this.OW, height: this.OH };
  }
  static side(v) { return ['left', 'right', 'top', 'bottom'].includes(v) ? v : 'right'; }

  _place(target) {
    for (let i = 0; i < 3; i++) {
      this.overlay.setBounds(target);
      const got = this.overlay.getBounds();
      if (Math.abs(got.x - target.x) <= 1 && Math.abs(got.y - target.y) <= 1 && got.width === target.width && got.height === target.height) return;
    }
  }

  positionOverlay(display = this._pillDisplay()) {
    if (!this.overlay) return;
    const a = this.store.config.appearance;
    this._fitScale(display);
    this._place(this._targetBounds(display, Windows.side(a.pillSide), a.pillY ?? 0.6));
  }

  syncOverlay(active = false) {
    if (!this.overlay) return;
    const keep = this.store.config.appearance.pillIdle !== false || this.pinned;
    if (keep || active) this.showOverlay(); else this.hideOverlay();
  }

  showOverlay() {
    if (!this.overlay) return;
    if (!this.overlayShown) this.positionOverlay();
    this.overlay.showInactive();
    this._raiseOverlay();
    this.overlayShown = true;
  }

  // Last raised topmost window wins, so re-assert on show and every 2s. The engine raises the pill over other
  // apps, then puts the taskbar back on top of it.
  _raiseOverlay() {
    const o = this.overlay;
    if (!o || o.isDestroyed()) return;
    o.setAlwaysOnTop(true, 'screen-saver');
    if (o.isVisible() && this.onRaise) this.onRaise(Number(o.getNativeWindowHandle().readBigUInt64LE(0)));
  }
  hideOverlay() {
    if (this.overlay && this.overlayShown) { this.overlayShown = false; this.overlay.hide(); }
  }

  dragOverlay(phase, p) {
    if (!this.overlay) return;
    if (phase === 'start') {
      const { x, y } = this.overlay.getBounds();
      this.drag = { dx: p.sx - x, dy: p.sy - y };
      this.overlay.setIgnoreMouseEvents(false);
    } else if (phase === 'move' && this.drag) {
      const b = this.overlay.getBounds();
      this.overlay.setBounds({ x: Math.round(p.sx - this.drag.dx), y: Math.round(p.sy - this.drag.dy), width: b.width, height: b.height });
    } else if (phase === 'end' && this.drag) {
      this.drag = null;
      this.overlay.setIgnoreMouseEvents(true, { forward: true });
      const cur = this.overlay.getBounds();
      const dx = typeof p.sx === 'number' ? p.sx : cur.x + cur.width / 2;
      const dy = typeof p.sy === 'number' ? p.sy : cur.y + cur.height / 2;
      const d = screen.getDisplayNearestPoint({ x: Math.round(dx), y: Math.round(dy) });
      const b = d.bounds, wa = d.workArea;
      const near = { left: (dx - b.x) / b.width, right: (b.x + b.width - dx) / b.width, top: (dy - b.y) / b.height, bottom: (b.y + b.height - dy) / b.height };
      const side = Object.keys(near).reduce((a, k) => (near[k] < near[a] ? k : a), 'right');
      this._fitScale(d);
      const yFrac = side === 'top' || side === 'bottom'
        ? Math.min(1, Math.max(0, (cur.x - wa.x) / Math.max(1, wa.width - this.OW)))
        : Math.min(1, Math.max(0, (cur.y - wa.y) / Math.max(1, wa.height - this.OH)));
      const target = this._targetBounds(d, side, yFrac);
      this.store.set('appearance.pillSide', side);
      this.store.set('appearance.pillY', Math.round(yFrac * 1000) / 1000);
      clearInterval(this._snap);
      this._snapping = true;
      let i = 0; const N = 8;
      this._snap = setInterval(() => {
        i++; const t = 1 - Math.pow(1 - i / N, 3);
        try {
          if (i < N) this.overlay.setBounds({ x: Math.round(cur.x + (target.x - cur.x) * t), y: Math.round(cur.y + (target.y - cur.y) * t), width: target.width, height: target.height });
          else { this._place(target); clearInterval(this._snap); this._snapping = false; }
        } catch { clearInterval(this._snap); this._snapping = false; }
      }, 16);
    }
  }

  createTray(handlers) {
    const img = nativeImage.createFromPath(BUILD('tray.png'));
    this.tray = new Tray(img);
    this.tray.setToolTip('Murmint');
    const menu = Menu.buildFromTemplate([
      { label: 'Open Murmint', click: () => this.createMain() },
      { label: 'Start / stop dictation', click: handlers.toggle },
      { type: 'separator' },
      { label: 'Quit', click: handlers.quit },
    ]);
    this.tray.setContextMenu(menu);
    this.tray.on('click', () => this.createMain());
  }

  broadcast(channel, payload) {
    for (const w of [this.main, this.overlay]) if (w && !w.isDestroyed()) w.webContents.send(channel, payload);
  }
  toOverlay(channel, payload) { if (this.overlay && !this.overlay.isDestroyed()) this.overlay.webContents.send(channel, payload); }
  toMain(channel, payload) { if (this.main && !this.main.isDestroyed()) this.main.webContents.send(channel, payload); }
}

module.exports = { Windows };
