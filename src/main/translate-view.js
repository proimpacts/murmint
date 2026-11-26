'use strict';
// Translation: a clear window over the screen with the translation in a bubble that points at the text it came from.
// Esc, Dismiss, a click outside, scrolling or leaving the window closes it.
const path = require('path');
const { BrowserWindow, screen, ipcMain, nativeTheme } = require('electron');

let win = null;

// Rects from the engine are physical pixels; the window works in DIPs
function toDip(r) {
  const d = screen.screenToDipRect(null, { x: Math.round(r[0]), y: Math.round(r[1]), width: Math.round(r[2]), height: Math.round(r[3]) });
  return [d.x, d.y, d.width, d.height];
}

function close() {
  if (win && !win.isDestroyed()) win.destroy();
  win = null;
}

async function show({ target, regions, anchor, accent }) {
  close();
  const cursor = screen.getCursorScreenPoint();
  const text = (regions || []).map((r) => String(r.translation || '').trim()).filter(Boolean).join('\n\n');
  // what the bubble points at: the paragraphs on screen, or else the box the text was copied from
  const rects = (regions || []).flatMap((r) => (r.rects || []).map(toDip));
  if (!rects.length && anchor) rects.push(toDip(anchor));
  let at = null;
  if (rects.length) {
    const x = Math.min(...rects.map((q) => q[0])), y = Math.min(...rects.map((q) => q[1]));
    at = { x, y, w: Math.max(...rects.map((q) => q[0] + q[2])) - x, h: Math.max(...rects.map((q) => q[1] + q[3])) - y };
  }
  // a whole page or editor is too big to sit next to, so then it points where you were pointing
  if (!at || at.h >= 260) at = { x: cursor.x, y: cursor.y, w: 0, h: 0 };
  const { bounds } = screen.getDisplayMatching({ x: at.x, y: at.y, width: Math.max(1, at.w), height: Math.max(1, at.h) });
  win = new BrowserWindow({
    ...bounds, show: false, frame: false, resizable: false, movable: false, skipTaskbar: true, alwaysOnTop: true,
    transparent: true, hasShadow: false, fullscreenable: false, maximizable: false, minimizable: false,
    webPreferences: { preload: path.join(__dirname, '..', 'preload.js'), contextIsolation: true, sandbox: true, spellcheck: false },
  });
  win.setAlwaysOnTop(true, 'pop-up-menu');
  win.setBounds(bounds);
  win.on('blur', close);
  win.webContents.once('did-finish-load', () => {
    if (!win) return;
    win.webContents.send('translate:bubble', {
      target, text, accent,
      theme: nativeTheme.shouldUseDarkColors ? 'dark' : 'light',
      at: { x: at.x - bounds.x, y: at.y - bounds.y, w: at.w, h: at.h },
    });
    win.show(); win.focus();
  });
  win.loadFile(path.join(__dirname, '..', 'renderer', 'translate', 'index.html'));
}

ipcMain.handle('translate:close', () => close());

module.exports = { show, close };
