'use strict';
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const exe = process.platform === 'win32' ? 'murmint-engine.exe' : 'murmint-engine';
const candidates = [
  process.env.MURMINT_ENGINE,
  path.join(root, 'bin', exe),
  path.join(root, 'engine', 'target', 'release', exe),
  path.join(root, 'engine', 'target', 'debug', exe),
].filter(Boolean);

const existing = candidates.find((p) => {
  try { return fs.statSync(p).isFile(); } catch { return false; }
});
if (existing) process.exit(0);

const res = spawnSync(process.execPath, [path.join(__dirname, 'build-engine.js')], { stdio: 'inherit' });
process.exit(res.status ?? 1);
