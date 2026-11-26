'use strict';
// In-place translation: a clear window over the screen that lays each translated paragraph over the original, in the
// page's own colours. Esc, a click outside, scrolling or leaving the window closes it; clicking a block shows the
// original under it.
const path = require('path');
const { BrowserWindow, desktopCapturer, screen, ipcMain } = require('electron');

let win = null;

// Rects from the engine are physical pixels; the window works in DIPs
function toDip(r) {
  const d = screen.screenToDipRect(null, { x: Math.round(r[0]), y: Math.round(r[1]), width: Math.round(r[2]), height: Math.round(r[3]) });
  return [d.x, d.y, d.width, d.height];
}

// Background of a block (and a text colour that reads on it), from a screenshot of its display taken before the window shows. The
// background is read just outside the block, so a selection's highlight isn't taken for the page.
function colours(img, scale, box) {
  const size = img.getSize(), bmp = img.toBitmap(); // BGRA
  const px = (x, y) => {
    x = Math.max(0, Math.min(size.width - 1, Math.round(x * scale))); y = Math.max(0, Math.min(size.height - 1, Math.round(y * scale)));
    const i = (y * size.width + x) * 4;
    return [bmp[i + 2], bmp[i + 1], bmp[i]];
  };
  const common = (list) => {
    const count = new Map();
    for (const c of list) { const k = c.map((v) => v >> 4).join(); count.set(k, (count.get(k) || 0) + 1); }
    const top = [...count.entries()].sort((a, b) => b[1] - a[1])[0][0];
    return list.find((c) => c.map((v) => v >> 4).join() === top);
  };
  const inside = [], ring = [];
  for (let i = 0; i <= 8; i++) for (let j = 0; j <= 4; j++) inside.push(px(box.x + (box.w * i) / 8, box.y + (box.h * j) / 4));
  const out = Math.max(4, box.line * 0.6);
  for (let i = 0; i <= 8; i++) ring.push(px(box.x + (box.w * i) / 8, box.y - out), px(box.x + (box.w * i) / 8, box.y + box.h + out));
  const dist = (a, b) => Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]) + Math.abs(a[2] - b[2]);
  const under = common(inside), around = common(ring);
  const bg = dist(under, around) > 60 ? around : under;
  const light = bg[0] * 0.3 + bg[1] * 0.59 + bg[2] * 0.11 > 140;
  return { bg: `rgb(${bg})`, fg: light ? 'rgb(24,24,24)' : 'rgb(240,240,240)' };
}

function close() {
  if (win && !win.isDestroyed()) win.destroy();
  win = null;
}

async function show({ target, card, regions, anchor }) {
  close();
  const cursor = screen.getCursorScreenPoint();
  const blocks = (regions || []).map((r) => {
    if (!r.rects || !r.rects.length) return { text: r.translation, original: r.text };
    const rs = r.rects.map(toDip);
    const x = Math.min(...rs.map((q) => q[0])), y = Math.min(...rs.map((q) => q[1]));
    const w = Math.max(...rs.map((q) => q[0] + q[2])) - x, h = Math.max(...rs.map((q) => q[1] + q[3])) - y;
    // a little taller than the lines themselves, to cover a selection's highlight around them
    const line = Math.min(...rs.map((q) => q[3])), pad = Math.round(line * 0.2);
    return { text: r.translation, original: r.text, x: x - 2, y: y - pad, w: w + 4, h: h + pad * 2, line };
  });
  const placed = blocks.filter((b) => b.w);
  // a card goes by the box the text was copied from (the mouse is only the last resort)
  const box = anchor ? toDip(anchor) : null;
  const display = placed.length
    ? screen.getDisplayMatching({ x: placed[0].x, y: placed[0].y, width: placed[0].w, height: placed[0].h })
    : box ? screen.getDisplayMatching({ x: box[0], y: box[1], width: box[2], height: box[3] }) : screen.getDisplayNearestPoint(cursor);
  const { bounds, scaleFactor } = display;
  if (placed.length) {
    try {
      const size = { width: Math.round(bounds.width * scaleFactor), height: Math.round(bounds.height * scaleFactor) };
      const sources = await desktopCapturer.getSources({ types: ['screen'], thumbnailSize: size });
      const src = sources.find((s) => String(s.display_id) === String(display.id)) || sources[0];
      for (const b of placed) Object.assign(b, colours(src.thumbnail, scaleFactor, { x: b.x - bounds.x, y: b.y - bounds.y, w: b.w, h: b.h, line: b.line }));
    } catch { }
  }
  for (const b of placed) { b.x -= bounds.x; b.y -= bounds.y; }
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
    win.webContents.send('translate:blocks', {
      target, card: !!card || !placed.length, blocks,
      cursor: { x: cursor.x - bounds.x, y: cursor.y - bounds.y },
      anchor: box ? { x: box[0] - bounds.x, y: box[1] - bounds.y, w: box[2], h: box[3] } : null,
    });
    win.show(); win.focus();
  });
  win.loadFile(path.join(__dirname, '..', 'renderer', 'translate', 'index.html'));
}

ipcMain.handle('translate:close', () => close());

module.exports = { show, close };
