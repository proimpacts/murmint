'use strict';
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const OUT = path.join(__dirname, '..', 'build');
fs.mkdirSync(OUT, { recursive: true });

function crc32(buf) {
  let c, crc = ~0;
  for (let n = 0; n < buf.length; n++) {
    c = (crc ^ buf[n]) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return ~crc >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
function png(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4); ihdr[8] = 8; ihdr[9] = 6;
  const raw = Buffer.alloc((size * 4 + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    rgba.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}

const sdRound = (px, py, cx, cy, hx, hy, r) => {
  const qx = Math.abs(px - cx) - (hx - r), qy = Math.abs(py - cy) - (hy - r);
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
};

function render(size, { background }) {
  const buf = Buffer.alloc(size * size * 4);
  const bars = [0.26, 0.5, 0.78, 0.5, 0.26];
  const accent = [255, 138, 91];
  const bg = [23, 24, 29];
  const ss = 4;
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    let r = 0, g = 0, b = 0, a = 0;
    for (let sy = 0; sy < ss; sy++) for (let sx = 0; sx < ss; sx++) {
      const px = (x + (sx + 0.5) / ss) / size, py = (y + (sy + 0.5) / ss) / size;
      let col = null, cov = 0;
      if (background) {
        const d = sdRound(px, py, 0.5, 0.5, 0.5, 0.5, 0.225);
        if (d < 0) { col = bg; cov = 1; }
      }
      const bw = background ? 0.075 : 0.085, gap = background ? 0.052 : 0.06;
      const total = bars.length * bw + (bars.length - 1) * gap;
      for (let i = 0; i < bars.length; i++) {
        const cx = 0.5 - total / 2 + bw / 2 + i * (bw + gap);
        const h = bars[i] * (background ? 0.62 : 0.9) / 2;
        if (sdRound(px, py, cx, 0.5, bw / 2, h, bw / 2) < 0) { col = accent; cov = 1; }
      }
      if (col) { r += col[0]; g += col[1]; b += col[2]; a += cov; }
    }
    const n = ss * ss, i = (y * size + x) * 4;
    if (a > 0) { buf[i] = Math.round(r / a); buf[i + 1] = Math.round(g / a); buf[i + 2] = Math.round(b / a); buf[i + 3] = Math.round((a / n) * 255); }
  }
  return buf;
}

const icon = png(256, render(256, { background: true }));
fs.writeFileSync(path.join(OUT, 'icon.png'), icon);
fs.writeFileSync(path.join(OUT, 'tray.png'), png(32, render(32, { background: false })));

const head = Buffer.alloc(22);
head.writeUInt16LE(0, 0); head.writeUInt16LE(1, 2); head.writeUInt16LE(1, 4);
head[6] = 0; head[7] = 0; head[8] = 0; head[9] = 0;
head.writeUInt16LE(1, 10); head.writeUInt16LE(32, 12);
head.writeUInt32LE(icon.length, 14); head.writeUInt32LE(22, 18);
fs.writeFileSync(path.join(OUT, 'icon.ico'), Buffer.concat([head, icon]));
console.log('icons written to', OUT);
