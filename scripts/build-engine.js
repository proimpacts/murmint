'use strict';
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const root = path.join(__dirname, '..');
// static CRT: no VC++ redistributable needed
const CRT = '-C target-feature=+crt-static';
const exe = process.platform === 'win32' ? 'murmint-engine.exe' : 'murmint-engine';
const env = { ...process.env, RUSTFLAGS: `${process.env.RUSTFLAGS || ''} ${CRT}`.trim(), PATH: `${path.join(os.homedir(), '.cargo', 'bin')}${path.delimiter}${process.env.PATH}` };

let res;
for (let i = 0; i < 3; i++) {
  res = spawnSync(process.platform === 'win32' ? 'cargo.exe' : 'cargo', ['build', '--release', '--manifest-path', path.join(root, 'engine', 'Cargo.toml')], { stdio: 'inherit', env });
  if (res.status === 0) break;
}
if (res.status !== 0) { console.error('\nEngine build failed. Install Rust (https://rustup.rs) and the Visual Studio C++ Build Tools.'); process.exit(1); }

fs.mkdirSync(path.join(root, 'bin'), { recursive: true });
fs.copyFileSync(path.join(root, 'engine', 'target', 'release', exe), path.join(root, 'bin', exe));
console.log('engine ready:', path.join('bin', exe), `(${Math.round(fs.statSync(path.join(root, 'bin', exe)).size / 1024)} KB)`);
