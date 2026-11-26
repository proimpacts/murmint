'use strict';
const { app, BrowserWindow } = require('electron');
const fs = require('fs');
const path = require('path');

function bmp24(image, w, h) {
  const img = image.resize({ width: w, height: h, quality: 'best' });
  const px = img.toBitmap();
  const row = Math.ceil((w * 3) / 4) * 4;
  const out = Buffer.alloc(54 + row * h);
  out.write('BM', 0); out.writeUInt32LE(out.length, 2); out.writeUInt32LE(54, 10);
  out.writeUInt32LE(40, 14); out.writeInt32LE(w, 18); out.writeInt32LE(h, 22); out.writeUInt16LE(1, 26); out.writeUInt16LE(24, 28);
  out.writeUInt32LE(row * h, 34); out.writeInt32LE(2835, 38); out.writeInt32LE(2835, 42);
  for (let y = 0; y < h; y++) {
    const src = y * w * 4, dst = 54 + (h - 1 - y) * row;
    for (let x = 0; x < w; x++) { out[dst + x * 3] = px[src + x * 4]; out[dst + x * 3 + 1] = px[src + x * 4 + 1]; out[dst + x * 3 + 2] = px[src + x * 4 + 2]; }
  }
  return out;
}

let win;
async function shot(file, w, h) {
  win ||= new BrowserWindow({ width: 400, height: 400, show: false, useContentSize: true, frame: false });
  await win.loadURL(require('url').pathToFileURL(path.join(__dirname, 'installer-art', file)).href);
  await new Promise((r) => setTimeout(r, 700));
  const image = await win.webContents.capturePage({ x: 0, y: 0, width: w, height: h });
  return bmp24(image, w, h);
}

app.whenReady().then(async () => {
  const dir = path.join(__dirname, '..', 'build');
  const side = await shot('sidebar.html', 164, 314);
  fs.writeFileSync(path.join(dir, 'installerSidebar.bmp'), side);
  fs.writeFileSync(path.join(dir, 'uninstallerSidebar.bmp'), side);
  fs.writeFileSync(path.join(dir, 'installerHeader.bmp'), await shot('header.html', 150, 57));
  console.log('installer art written to build/');
  app.quit();
});
