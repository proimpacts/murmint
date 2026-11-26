'use strict';
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const root = path.join(__dirname, '..');
// static CRT: no VC++ redistributable needed
const CRT = '-C target-feature=+crt-static';
const { version } = require('../package.json');
const core = path.join(root, 'dist', `Murmint-core-${version}.exe`);
if (!fs.existsSync(core)) { console.error('Missing', core, '- run electron-builder first.'); process.exit(1); }

const size = (p) => fs.statSync(p).isDirectory() ? fs.readdirSync(p).reduce((n, f) => n + size(path.join(p, f)), 0) : fs.statSync(p).size;
const unpacked = path.join(root, 'dist', 'win-unpacked');
const bytes = fs.existsSync(unpacked) ? size(unpacked) : 300e6;

const env = { ...process.env, RUSTFLAGS: `${process.env.RUSTFLAGS || ''} ${CRT}`.trim(), PATH: `${path.join(os.homedir(), '.cargo', 'bin')}${path.delimiter}${process.env.PATH}`, MURMINT_PAYLOAD: core, MURMINT_UNPACKED_BYTES: String(bytes), MURMINT_VERSION: version };
const release = path.join(root, 'installer', 'target', 'release');
const cargo = (bin) => spawnSync('cargo', ['build', '--release', '--bin', bin, '--manifest-path', path.join(root, 'installer', 'Cargo.toml')], { stdio: 'inherit', env, shell: true }).status === 0;
// setup embeds the uninstaller
if (!cargo('murmint-uninstall')) { console.error('\nUninstaller build failed.'); process.exit(1); }
env.MURMINT_UNINSTALLER = path.join(release, 'murmint-uninstall.exe');
if (!cargo('murmint-setup')) { console.error('\nSetup build failed.'); process.exit(1); }

const out = path.join(root, 'dist', `Murmint Setup ${version}.exe`);
fs.copyFileSync(path.join(release, 'murmint-setup.exe'), out);
console.log('setup ready:', path.relative(root, out), `(${Math.round(fs.statSync(out).size / 1048576)} MB)`);
