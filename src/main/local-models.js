'use strict';
// Murmint 1, the speech model everyone gets: OpenAI's Whisper Tiny (MIT license) run by whisper.cpp on this computer,
// so speech to text costs nothing. Formatting goes to the providers the person adds.
// The model and the server are pinned to one version and checked against their SHA-256 before they are used.
//
// The server listens on 127.0.0.1 only, behind a small proxy that wants a key made fresh each launch, so other
// programs and web pages on this computer can't use it. The engine sees one OpenAI-style provider.
const fs = require('fs');
const os = require('os');
const path = require('path');
const http = require('http');
const crypto = require('crypto');
const { spawn, execFile } = require('child_process');
const { app, net } = require('electron');

const PROVIDER = 'murmint-local';
const STT_MODEL = 'whisper-tiny';
const GB = 1024 ** 3;

const CPU_VARIANTS = ['alderlake', 'cannonlake', 'cascadelake', 'haswell', 'icelake', 'sandybridge', 'skylakex', 'sse42', 'x64'].map((n) => `ggml-cpu-${n}.dll`);
const PARTS = [
  {
    id: 'whisper', label: 'Speech engine', dir: 'whisper',
    url: 'https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip',
    size: 8928640, sha256: '6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb',
    strip: 1,
    files: ['whisper-server.exe', 'whisper.dll', 'ggml.dll', 'ggml-base.dll', ...CPU_VARIANTS].map((f) => 'Release/' + f),
  },
  {
    id: 'stt', label: 'Murmint 1', file: 'ggml-tiny-q5_1.bin',
    url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny-q5_1.bin',
    size: 32152673, sha256: '818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7',
  },
];
const TOTAL = PARTS.reduce((n, p) => n + p.size, 0);
const SPARE = 50 * 1024 * 1024; // room for the zip while it unpacks

const root = () => path.join(app.getPath('userData'), 'models');
const at = (...p) => path.join(root(), ...p);
const done = (p) => { try { return fs.readFileSync(at(p.id + '.ok'), 'utf8') === p.sha256; } catch { return false; } };
const installed = () => PARTS.every(done);
const gb = (n) => (n / GB).toFixed(1) + ' GB';

async function freeSpace() {
  try {
    fs.mkdirSync(root(), { recursive: true });
    const s = await fs.promises.statfs(root());
    return s.bavail * s.bsize;
  } catch { return null; }
}

// ---- download ----

let job = null;
let progress = { state: 'none', received: 0, total: TOTAL, error: null, checking: false };
const watchers = new Set();
function report(patch) {
  progress = { ...progress, ...patch };
  for (const fn of watchers) { try { fn(progress); } catch { } }
}

function status() {
  return { ...progress, state: job ? 'downloading' : installed() ? 'installed' : progress.state, installed: installed(), running: !!(whisper && whisper.ready), size: TOTAL };
}

async function sha256(file) {
  const h = crypto.createHash('sha256');
  for await (const chunk of fs.createReadStream(file)) h.update(chunk);
  return h.digest('hex');
}

// Downloads one file, picking up where an earlier try stopped.
async function fetchTo(p, file, base, signal) {
  const have = fs.existsSync(file) ? fs.statSync(file).size : 0;
  if (have >= p.size) return;
  const r = await net.fetch(p.url, { signal, cache: 'no-store', headers: have ? { Range: `bytes=${have}-` } : {} });
  if (!r.ok) throw new Error(`the download server answered ${r.status}`);
  const append = have > 0 && r.status === 206;
  const out = fs.createWriteStream(file, { flags: append ? 'a' : 'w' });
  let got = append ? have : 0, shown = 0;
  try {
    const reader = r.body.getReader();
    for (;;) {
      const { done: end, value } = await reader.read();
      if (end) break;
      got += value.length;
      if (got > p.size) throw new Error('the download was bigger than expected');
      if (!out.write(value)) await new Promise((res) => out.once('drain', res));
      if (Date.now() - shown > 250) { shown = Date.now(); report({ received: base + got }); }
    }
  } finally {
    await new Promise((res) => out.end(res));
  }
}

const TAR = path.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'tar.exe');
function unzip(zip, dest, p) {
  fs.mkdirSync(dest, { recursive: true });
  const args = ['-xf', zip, '-C', dest, '--strip-components', String(p.strip), ...p.files];
  return new Promise((res, rej) => execFile(TAR, args, { windowsHide: true }, (e) => (e ? rej(new Error('could not unpack ' + p.label)) : res())));
}

async function install(p, base, signal) {
  const file = at(p.file || p.id + '.zip.part');
  const tmp = p.file ? file + '.part' : file;
  await fetchTo(p, tmp, base, signal);
  report({ checking: true });
  const sum = await sha256(tmp);
  report({ checking: false });
  if (sum !== p.sha256) {
    fs.rmSync(tmp, { force: true });
    throw new Error(`${p.label} did not download correctly. Try again.`);
  }
  if (p.files) {
    await unzip(tmp, at(p.dir), p);
    fs.rmSync(tmp, { force: true });
  } else fs.renameSync(tmp, file);
  fs.writeFileSync(at(p.id + '.ok'), p.sha256);
}

// Downloads Murmint 1. `onDone` runs once it is in place.
async function download(onDone) {
  if (job) return status();
  if (installed()) { if (onDone) onDone(); return status(); }
  const left = PARTS.filter((p) => !done(p)).reduce((n, p) => n + p.size, 0);
  const free = await freeSpace();
  if (free != null && free < left + SPARE) {
    report({ state: 'error', error: `Not enough disk space. Murmint 1 needs ${gb(left + SPARE)} free and this drive has ${gb(free)}.` });
    return status();
  }
  const ctl = new AbortController();
  job = ctl;
  report({ state: 'downloading', error: null, received: 0 });
  (async () => {
    try {
      let base = 0;
      for (const p of PARTS) {
        if (!done(p)) await install(p, base, ctl.signal);
        base += p.size;
        report({ received: base });
      }
      job = null;
      report({ state: 'installed' });
      if (onDone) onDone();
    } catch (e) {
      job = null;
      report(ctl.signal.aborted ? { state: 'none' } : { state: 'error', error: e.message || String(e) });
    }
  })();
  return status();
}

function cancel() { if (job) job.abort(); }

// ---- running it ----

let proxy = null, key = '', stopping = false, whisper = null, core = null;

function freePort() {
  return new Promise((res, rej) => {
    const s = require('net').createServer();
    s.unref(); s.on('error', rej);
    s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
  });
}

async function waitFor(port) {
  const until = Date.now() + 60000;
  while (Date.now() < until && !stopping) {
    const ok = await new Promise((res) => {
      const rq = http.get({ host: '127.0.0.1', port, path: '/', timeout: 2000 }, (r) => { r.resume(); res(r.statusCode === 200); });
      rq.on('error', () => res(false)); rq.on('timeout', () => { rq.destroy(); res(false); });
    });
    if (ok) return true;
    await new Promise((r) => setTimeout(r, 250));
  }
  return false;
}

// Runs the speech server; one that crashes is restarted a few times.
async function launch() {
  const port = await freePort();
  const exe = at('whisper', 'whisper-server.exe');
  const threads = String(Math.max(2, Math.floor((os.availableParallelism ? os.availableParallelism() : os.cpus().length) / 2)));
  let tries = 0;
  const run = () => {
    const child = spawn(exe, ['-m', at(PARTS[1].file), '--host', '127.0.0.1', '--port', String(port), '-l', 'auto', '-t', threads, '--inference-path', '/v1/audio/transcriptions'],
      { cwd: path.dirname(exe), windowsHide: true, stdio: 'ignore' });
    const me = whisper = { child, port, ready: false };
    child.on('exit', () => {
      me.ready = false;
      if (whisper !== me || stopping) return;
      if (++tries <= 3) setTimeout(run, 1500 * tries);
    });
    me.wait = waitFor(port).then((ok) => { me.ready = ok; return ok; });
  };
  run();
  return whisper.wait;
}

function serveProxy() {
  return new Promise((res) => {
    proxy = http.createServer((req, rsp) => {
      const fail = (code, message) => { if (!rsp.headersSent) rsp.writeHead(code, { 'Content-Type': 'application/json' }); rsp.end(JSON.stringify({ error: { message } })); };
      const given = Buffer.from(String(req.headers.authorization || ''));
      const want = Buffer.from('Bearer ' + key);
      if (given.length !== want.length || !crypto.timingSafeEqual(given, want)) return fail(401, 'Wrong key');
      const route = (req.url || '').split('?')[0];
      if (route === '/v1/models') {
        rsp.writeHead(200, { 'Content-Type': 'application/json' });
        return rsp.end(JSON.stringify({ object: 'list', data: [{ id: STT_MODEL, object: 'model' }] }));
      }
      if (route !== '/v1/audio/transcriptions') return fail(404, 'Not found');
      if (!whisper || !whisper.ready) return fail(503, 'Murmint 1 is still starting. Try again in a moment.');
      const up = http.request({ host: '127.0.0.1', port: whisper.port, path: req.url, method: req.method, headers: { ...req.headers, host: '127.0.0.1:' + whisper.port } },
        (u) => { rsp.writeHead(u.statusCode, u.headers); u.pipe(rsp); });
      up.on('error', () => fail(502, 'Murmint 1 stopped. It restarts on its own; try again in a moment.'));
      req.pipe(up);
    });
    proxy.listen(0, '127.0.0.1', () => res(proxy.address().port));
  });
}

// Files earlier versions downloaded for a formatting model that runs on this computer, which Murmint no longer has.
function dropLegacy() {
  for (const f of ['llama', 'llama.ok', 'Qwen2.5-Omni-3B-Q4_K_M.gguf', 'format.ok', 's1-mini-q4_k_m.gguf', 's1-mini.ok',
    'SmolLM2-135M-Instruct-Q8_0.gguf', 'smollm2-135m.ok', 'gemma-3-270m-it-Q8_0.gguf', 'gemma-3-270m.ok', 'bench.json']) {
    try { fs.rmSync(at(f), { recursive: true, force: true }); fs.rmSync(at(f + '.part'), { force: true }); } catch { }
  }
}

// Starts Murmint 1 and points the engine at it; resolves true once it answers.
let starting = null;
function start(engine) {
  core = engine;
  dropLegacy();
  if (!installed()) return Promise.resolve(false);
  if (!starting) starting = boot().then((ok) => { if (!ok) stop(); return ok; }, (e) => { stop(); throw e; });
  return starting;
}

async function boot() {
  stopping = false;
  key = crypto.randomBytes(24).toString('hex');
  const port = await serveProxy();
  await core.ui('provider:save', {
    id: PROVIDER, name: 'Murmint 1', baseUrl: `http://127.0.0.1:${port}/v1`, keyOptional: true, local: true, free: true,
    note: 'Murmint’s own speech model, running on this computer. Free, private and works offline.',
    models: [{ id: STT_MODEL, label: 'Murmint 1', task: 'stt', tier: 'bad', price: { perMinute: 0 } }],
  });
  await core.ui('provider:key', PROVIDER, key);
  return launch();
}

// Makes Murmint 1 the speech model, keeping what the person had as a fallback.
async function use(engine) {
  if (!(await start(engine))) return false;
  const was = (await core.ui('state:get')).config.models.stt;
  const keep = [was.primary, ...(was.fallbacks || [])].filter((r) => r && !r.startsWith(PROVIDER + '::'));
  await core.ui('config:set', 'models.stt', { primary: `${PROVIDER}::${STT_MODEL}`, fallbacks: [...new Set(keep)].slice(0, 3), routing: 'manual' });
  return true;
}

function stop() {
  stopping = true; starting = null;
  const w = whisper; whisper = null;
  if (w) { try { w.child.kill(); } catch { } }
  if (proxy) { proxy.close(); proxy = null; }
}

function watch(fn) { watchers.add(fn); return () => watchers.delete(fn); }

module.exports = { PROVIDER, STT_MODEL, status, download, cancel, start, stop, use, watch, installed, freeSpace };
