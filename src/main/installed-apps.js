'use strict';
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { app, shell, dialog, nativeImage } = require('electron');

// not apps: uninstallers, help, release notes
const SKIP_NAME = /uninstall|readme|read me|release notes|licen[sc]e|documentation|\bhelp\b|manual|web ?site|\bvisit\b|support|changelog|what's new|getting started|tutorial|\bsdk\b|command prompt|safe mode|repair|\bsetup\b|installer|updater|check for updates|\babout\b|manageab|\bservice\b|\bhost\b|reload|configur|install manager|stack builder|cert kit|diagnos|troubleshoot|event utility|\bdefender\b/i;
const SKIP_PROC = /^(unins\d*|uninstall.*|update|updater|installer|setup|helper|.*_proxy|mmc|control|rundll32|msiexec|explorer|cscript|wscript|regedit|taskmgr|dllhost|applicationframehost)$/i;
// Windows tools worth keeping
const SYSTEM_OK = new Set(['notepad', 'cmd', 'powershell', 'mspaint', 'wordpad']);

const SYSTEM_PACKAGES = ['microsoft.windows.', 'microsoftwindows.', 'windows.', 'microsoft.xbox', 'microsoft.gamingapp', 'microsoft.mixedreality',
  'microsoftcorporationii.quickassist', 'microsoftcorporationii.microsoftfamily', 'microsoft.getstarted', 'microsoft.gethelp', 'microsoft.windowsstore',
  'microsoft.sechealthui', 'microsoft.ppiprojection', 'microsoft.windowsappruntime', 'microsoft.549981c3f5f10'];

const norm = (p) => path.basename(String(p || '')).replace(/\.exe$/i, '').toLowerCase();
const byName = (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });

const cache = { classic: null, store: null, icons: new Map() };

function walk(dir, depth, out) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const e of entries) {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) { if (depth > 0) walk(full, depth - 1, out); }
    else if (/\.lnk$/i.test(e.name)) out.push(full);
  }
}

function describe(lnk, winDir) {
  const name = path.basename(lnk, '.lnk');
  if (SKIP_NAME.test(name)) return null;
  let d;
  try { d = shell.readShortcutLink(lnk); } catch { return null; }
  const target = d.target || '';
  if (!/\.exe$/i.test(target) || !fs.existsSync(target)) return null;
  let proc = norm(target);
  if (proc === 'update') {
    // Squirrel: Update.exe --processStart <exe>
    const m = /--processStart\s+"?([^\s"]+\.exe)"?/i.exec(d.args || '');
    if (!m) return null;
    proc = norm(m[1]);
  }
  if (SKIP_PROC.test(proc) || proc === 'murmint') return null;
  if (target.toLowerCase().startsWith(winDir) && !SYSTEM_OK.has(proc)) return null;
  return { name, process: proc, iconPath: d.icon && fs.existsSync(d.icon) ? d.icon : target };
}

async function iconOf(file) {
  if (cache.icons.has(file)) return cache.icons.get(file);
  let url = null;
  try {
    let img = /\.(ico|png)$/i.test(file) ? nativeImage.createFromPath(file) : await app.getFileIcon(file, { size: 'normal' });
    if (!img.isEmpty()) url = img.resize({ width: 32, height: 32 }).toDataURL();
  } catch { /* no icon for this one: the page shows a plain placeholder */ }
  cache.icons.set(file, url);
  return url;
}

async function withIcons(list) {
  const out = [];
  for (let i = 0; i < list.length; i += 8) {
    const chunk = list.slice(i, i + 8);
    const icons = await Promise.all(chunk.map((a) => iconOf(a.iconPath)));
    chunk.forEach((a, k) => out.push({ name: a.name, process: a.process, icon: icons[k] }));
  }
  return out;
}

function unique(list) {
  const seen = new Map();
  for (const a of list) if (!seen.has(a.process) || a.name.length < seen.get(a.process).name.length) seen.set(a.process, a);
  return [...seen.values()].sort(byName);
}

/** Programs with a Start Menu shortcut: the ones you would call "apps". */
async function classic() {
  if (cache.classic) return cache.classic;
  const lnks = [];
  for (const base of [process.env.ProgramData, process.env.APPDATA]) {
    if (base) walk(path.join(base, 'Microsoft', 'Windows', 'Start Menu', 'Programs'), 3, lnks);
  }
  const winDir = (process.env.WINDIR || 'C:\\Windows').toLowerCase();
  const found = unique(lnks.map((l) => describe(l, winDir)).filter(Boolean));
  cache.classic = await withIcons(found);
  return cache.classic;
}

const STORE_SCRIPT = `
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$places = @{}
Get-AppxPackage | ForEach-Object { $places[$_.PackageFamilyName] = $_.InstallLocation }
$found = foreach ($a in Get-StartApps) {
  if ($a.AppID -notmatch '!') { continue }
  $family, $id = $a.AppID -split '!', 2
  $where = $places[$family]
  if (-not $where) { continue }
  $manifest = Join-Path $where 'AppxManifest.xml'
  if (-not (Test-Path -LiteralPath $manifest)) { continue }
  $xml = [xml](Get-Content -LiteralPath $manifest -Raw)
  $node = $xml.Package.Applications.Application | Where-Object { $_.Id -eq $id } | Select-Object -First 1
  if (-not $node -or -not $node.Executable) { continue }
  $logo = $null
  $rel = $node.VisualElements.Square44x44Logo
  if ($rel) {
    $dir = Join-Path $where (Split-Path $rel -Parent)
    $base = [IO.Path]::GetFileNameWithoutExtension($rel)
    $logo = Get-ChildItem -LiteralPath $dir -Filter "$base*.png" | Where-Object { $_.Name -notmatch 'contrast|unplated' } | Sort-Object Length -Descending | Select-Object -First 1 -ExpandProperty FullName
  }
  [pscustomobject]@{ name = $a.Name; family = $family; exe = (Join-Path $where $node.Executable); logo = $logo }
}
ConvertTo-Json -Compress -InputObject @($found)
`;

/** Apps installed from the Microsoft Store. Slower to find, so the page asks for these separately. */
function store() {
  if (cache.store) return Promise.resolve(cache.store);
  return new Promise((resolve) => {
    execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', STORE_SCRIPT],
      { windowsHide: true, timeout: 60000, maxBuffer: 16 << 20 }, async (err, stdout) => {
        let list = [];
        try { list = err ? [] : JSON.parse(stdout || '[]'); } catch { list = []; }
        const apps = unique(list
          .filter((a) => a && /\.exe$/i.test(a.exe || '') && !SKIP_NAME.test(a.name) && !SKIP_PROC.test(norm(a.exe))
            && !SYSTEM_PACKAGES.some((p) => String(a.family || '').toLowerCase().startsWith(p)))
          .map((a) => ({ name: a.name, process: norm(a.exe), iconPath: a.logo || a.exe })));
        cache.store = await withIcons(apps);
        resolve(cache.store);
      });
  });
}

/** For a program that has no Start Menu entry (a portable one): pick its file. */
async function pick(win) {
  const r = await dialog.showOpenDialog(win || undefined, {
    title: 'Choose a program',
    properties: ['openFile'],
    filters: [{ name: 'Programs', extensions: ['exe'] }],
  });
  if (r.canceled || !r.filePaths[0]) return null;
  const file = r.filePaths[0];
  return { name: path.basename(file).replace(/\.exe$/i, ''), process: norm(file), icon: await iconOf(file) };
}

module.exports = { classic, store, pick };
