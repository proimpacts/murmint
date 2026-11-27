'use strict';
const { describe, it, afterEach } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const Module = require('node:module');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');

const ROOT = path.join(__dirname, '..');

function loadRecorder(overrides = {}) {
  const src = fs.readFileSync(path.join(ROOT, 'src', 'renderer', 'overlay', 'recorder.js'), 'utf8');
  const tracks = [];
  let workletNode = null;
  let nowMs = 1000;

  const createStream = () => {
    const track = { readyState: 'live', stopped: false, stop() { this.stopped = true; this.readyState = 'ended'; } };
    tracks.push(track);
    return { getAudioTracks: () => [track], getTracks: () => [track] };
  };

  class FakeAudioWorkletNode {
    constructor() {
      this.port = { onmessage: null };
      workletNode = this;
    }
  }

  class FakeAudioContext {
    constructor() {
      this.state = 'running';
      this.closed = false;
      this.audioWorklet = { addModule: async () => {} };
    }
    async resume() { this.state = 'running'; }
    createMediaStreamSource() { return { connect() {}, disconnect() {} }; }
    close() { this.closed = true; }
  }

  const windowObj = {};
  const ctx = vm.createContext({
    window: windowObj,
    performance: { now: () => nowMs },
    navigator: {
      mediaDevices: {
        getUserMedia: overrides.getUserMedia || (async () => createStream()),
      },
    },
    AudioContext: FakeAudioContext,
    AudioWorkletNode: FakeAudioWorkletNode,
    Int16Array,
    Float32Array,
    ArrayBuffer,
    DataView,
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
  });
  vm.runInContext(src, ctx, { filename: 'recorder.js' });
  return {
    Recorder: windowObj.Recorder,
    tracks,
    getWorklet: () => workletNode,
    getNow: () => nowMs,
    setNow: (v) => { nowMs = v; },
    createStream,
  };
}

function loadBridge() {
  const src = fs.readFileSync(path.join(ROOT, 'src', 'renderer', 'shared', 'bridge.js'), 'utf8');
  let deferred = [];
  let rafCalls = 0;
  let acCreated = 0;
  let acClosed = 0;
  const levels = [];

  class FakeAudioContext {
    constructor() { acCreated++; }
    createAnalyser() {
      return { fftSize: 512, getFloatTimeDomainData(buf) { buf.fill(0.1); } };
    }
    createMediaStreamSource() { return { connect() {} }; }
    close() { acClosed++; }
  }

  const windowObj = {
    murmintIpc: { on: () => () => {}, winAction: () => {} },
  };
  const ctx = vm.createContext({
    window: windowObj,
    document: { documentElement: { classList: { add() {} } } },
    navigator: {
      mediaDevices: {
        getUserMedia: () => new Promise((resolve, reject) => { deferred.push({ resolve, reject }); }),
        enumerateDevices: async () => [],
      },
    },
    AudioContext: FakeAudioContext,
    Float32Array,
    requestAnimationFrame: () => { rafCalls++; return rafCalls; },
    cancelAnimationFrame: () => {},
  });
  vm.runInContext(src, ctx, { filename: 'bridge.js' });
  windowObj.murmint.on('mic:level', (v) => levels.push(v));
  return {
    murmint: windowObj.murmint,
    deferred,
    stats: () => ({ rafCalls, acCreated, acClosed, levels }),
  };
}

function withBetterClipboardMocks(fn) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'murmint-bc-test-'));
  const origLoad = Module._load;
  const bcPath = require.resolve('../src/main/better-clipboard.js');
  delete require.cache[bcPath];

  const state = {
    regCalls: [],
    regSyncCalls: [],
    regStore: {},
    fetchFn: async () => { throw new Error('unexpected net.fetch'); },
    clipboardImage: { isEmpty: () => true, getSize: () => ({ width: 0 }), resize() { return this; }, toPNG: () => Buffer.alloc(0) },
    writtenImages: [],
    callLog: [],
  };

  const fakeElectron = {
    app: { getPath: () => tmpDir },
    BrowserWindow: class {},
    clipboard: {
      readImage: () => state.clipboardImage,
      writeImage: (img) => {
        state.callLog.push('clipboard:writeImage');
        state.writtenImages.push(img);
      },
    },
    nativeImage: {
      createFromPath: (p) => {
        const buf = fs.existsSync(p) ? fs.readFileSync(p) : Buffer.alloc(0);
        return { isEmpty: () => buf.length === 0, path: p, buf };
      },
    },
    net: {
      fetch: (...args) => state.fetchFn(...args),
    },
    screen: {},
  };

  const fakeChildProcess = {
    execFile: (cmd, args, opts, cb) => {
      state.regCalls.push({ cmd, args: [...args] });
      if (args[0] === 'query') {
        const val = state.regStore[`${args[1]}\\${args[3]}`];
        if (val == null) return cb(new Error('not found'), '');
        return cb(null, `    ${args[3]}    REG_DWORD    0x${val.toString(16)}\r\n`);
      }
      if (args[0] === 'add') {
        const dIdx = args.indexOf('/d');
        state.regStore[`${args[1]}\\${args[3]}`] = Number(args[dIdx + 1]);
        return cb(null, '');
      }
      if (args[0] === 'delete') {
        delete state.regStore[`${args[1]}\\${args[3]}`];
        return cb(null, '');
      }
      cb(null, '');
    },
    execFileSync: (cmd, args) => {
      state.regSyncCalls.push({ cmd, args: [...args] });
      if (args[0] === 'query') {
        const val = state.regStore[`${args[1]}\\${args[3]}`];
        if (val == null) throw new Error('not found');
        return `    ${args[3]}    REG_DWORD    0x${val.toString(16)}\r\n`;
      }
      if (args[0] === 'add') {
        const dIdx = args.indexOf('/d');
        state.regStore[`${args[1]}\\${args[3]}`] = Number(args[dIdx + 1]);
        return '';
      }
      if (args[0] === 'delete') {
        delete state.regStore[`${args[1]}\\${args[3]}`];
        return '';
      }
      return '';
    },
  };

  Module._load = function (request, parent, isMain) {
    if (request === 'electron') return fakeElectron;
    if (request === 'child_process') return fakeChildProcess;
    return origLoad.apply(this, arguments);
  };

  const bc = require('../src/main/better-clipboard.js');
  const cleanup = () => {
    Module._load = origLoad;
    delete require.cache[bcPath];
    fs.rmSync(tmpDir, { recursive: true, force: true });
  };

  return Promise.resolve()
    .then(() => fn({ bc, state, tmpDir, clipDir: path.join(tmpDir, 'data', 'clipboard') }))
    .finally(cleanup);
}

describe('Recorder (M09, U4)', () => {
  it('clears pre-roll ring and releases mic on cancel so cold start emits zero stale chunks (M09)', async () => {
    const chunks = [];
    const env = loadRecorder();
    const rec = new env.Recorder({
      onLevel: () => {},
      onChunk: (id, buf) => chunks.push({ id, len: buf.byteLength }),
      onSegment: () => {},
      onPreview: async () => '',
      onCaption: () => {},
      onBoundary: () => {},
      onAutoStop: () => {},
      onError: () => {},
    });

    assert.equal(await rec.start({ captureId: 1, stream: true }), true);
    const w1 = env.getWorklet();
    rec.stop(); // keeps mic warm while recording=false
    const stale = new Float32Array(1024).fill(0.25);
    w1.port.onmessage({ data: stale });
    assert.equal(rec.ring.length, 1);

    rec.cancel();
    assert.equal(rec.ring.length, 0);
    assert.equal(env.tracks[0].stopped, true);

    chunks.length = 0;
    assert.equal(await rec.start({ captureId: 2, stream: true }), true);
    assert.equal(chunks.length, 0);
  });

  it('resets lastSpeechAt on resume() so long pauses do not trigger immediate autoStop (U4)', async () => {
    let autoStopped = 0;
    const env = loadRecorder();
    const rec = new env.Recorder({
      onLevel: () => {},
      onChunk: () => {},
      onSegment: () => {},
      onPreview: async () => '',
      onCaption: () => {},
      onBoundary: () => {},
      onAutoStop: () => { autoStopped++; },
      onError: () => {},
    });
    rec.clock = env.getNow;

    assert.equal(await rec.start({ captureId: 1, stream: true, autoStopMs: 1000 }), true);
    rec.pause();
    env.setNow(10000); // pause longer than autoStopMs (1000ms)
    rec.resume();
    assert.equal(rec.paused, false);
    assert.equal(rec.lastSpeechAt, 13000);

    const silent = new Float32Array(1024).fill(0);
    env.getWorklet().port.onmessage({ data: silent });
    assert.equal(autoStopped, 0);

    env.setNow(14100); // > 13000 + 1000
    env.getWorklet().port.onmessage({ data: silent });
    assert.equal(autoStopped, 1);
  });
});

describe('bridge.js micTest (M10)', () => {
  it('stops stream tracks and avoids AudioContext/rAF if stopped before getUserMedia resolves', async () => {
    const { murmint, deferred, stats } = loadBridge();
    const pending = murmint.micTest(true);
    murmint.micTest(false);

    const track = { stopped: false, stop() { this.stopped = true; } };
    deferred[0].resolve({ getTracks: () => [track] });
    await pending;

    assert.equal(track.stopped, true);
    assert.equal(stats().acCreated, 0);
    assert.equal(stats().rafCalls, 0);
  });

  it('stops earlier stream when micTest(true) is restarted before first permission resolves', async () => {
    const { murmint, deferred, stats } = loadBridge();
    const p1 = murmint.micTest(true);
    const p2 = murmint.micTest(true);

    const t1 = { stopped: false, stop() { this.stopped = true; } };
    const t2 = { stopped: false, stop() { this.stopped = true; } };
    deferred[0].resolve({ getTracks: () => [t1] });
    deferred[1].resolve({ getTracks: () => [t2] });
    await Promise.all([p1, p2]);

    assert.equal(t1.stopped, true);
    assert.equal(t2.stopped, false);
    assert.equal(stats().acCreated, 1);
    assert.equal(stats().rafCalls, 1);
    murmint.micTest(false);
    assert.equal(t2.stopped, true);
    assert.equal(stats().acClosed, 1);
  });
});

describe('better-clipboard.js (M11, M12, M25, M31, M34)', () => {
  const origPlatform = Object.getOwnPropertyDescriptor(process, 'platform');
  afterEach(() => {
    if (origPlatform) Object.defineProperty(process, 'platform', origPlatform);
  });

  it('stores kind: "files" and replays via clipboard:paste_file (M11)', async () => {
    await withBetterClipboardMocks(async ({ bc, state }) => {
      const fakeCore = {
        async ui(ch, payload) {
          state.callLog.push({ ch, payload });
          return true;
        },
      };
      await bc.enable(fakeCore, false);
      bc.copied({ files: ['C:\\docs\\a.txt', 'C:\\docs\\b.txt'] });
      bc.copied({ files: ['C:\\docs\\a.txt', 'C:\\docs\\b.txt'] });

      const items = bc.list();
      assert.equal(items.length, 1);
      assert.equal(items[0].kind, 'files');
      assert.deepEqual(items[0].files, ['C:\\docs\\a.txt', 'C:\\docs\\b.txt']);

      await bc.pick(items[0].id);
      assert.deepEqual(state.callLog.at(-1), {
        ch: 'clipboard:paste_file',
        payload: { hwnd: 0, paths: ['C:\\docs\\a.txt', 'C:\\docs\\b.txt'] },
      });
    });
  });

  it('deduplicates repeated GIFs without leaving orphan files and sweeps orphans on clear/load (M12)', async () => {
    await withBetterClipboardMocks(async ({ bc, state, clipDir }) => {
      const gifBytes = Buffer.from('GIF89a\x01\x00\x01\x00\x00\x00\x00;');
      let fetches = 0;
      state.fetchFn = async () => {
        fetches++;
        return {
          ok: true,
          headers: new Headers({ 'content-length': String(gifBytes.length) }),
          body: new ReadableStream({
            start(ctrl) { ctrl.enqueue(gifBytes); ctrl.close(); },
          }),
        };
      };

      await bc.copied({ gif: 'https://example.com/party.gif' });
      await bc.copied({ gif: 'https://example.com/party.gif' });
      assert.equal(fetches, 1);
      assert.equal(bc.list().length, 1);

      const filesBeforeClear = fs.readdirSync(clipDir).filter((f) => f.endsWith('.gif'));
      assert.equal(filesBeforeClear.length, 1);

      fs.writeFileSync(path.join(clipDir, 'orphan.png'), 'x');
      bc.clear();
      const mediaAfterClear = fs.readdirSync(clipDir).filter((f) => /\.(gif|png)$/i.test(f));
      assert.equal(mediaAfterClear.length, 0);
    });
  });

  it('aborts chunked GIF downloads immediately past GIF_MAX and rejects non-ok / large content-length (M25)', async () => {
    await withBetterClipboardMocks(async ({ bc, state, clipDir }) => {
      state.fetchFn = async () => ({ ok: false, headers: new Headers(), body: null });
      const r1 = await bc.copied({ gif: 'https://example.com/404.gif' });
      assert.equal(r1, null);

      state.fetchFn = async () => ({
        ok: true,
        headers: new Headers({ 'content-length': String(16 * 1024 * 1024) }),
        body: new ReadableStream({ start(ctrl) { ctrl.close(); } }),
      });
      const r2 = await bc.copied({ gif: 'https://example.com/header-huge.gif' });
      assert.equal(r2, null);

      let pulls = 0;
      let aborted = false;
      state.fetchFn = async (_url, opts) => {
        opts.signal.addEventListener('abort', () => { aborted = true; });
        const chunk = Buffer.alloc(4 * 1024 * 1024, 0x41);
        chunk.write('GIF', 0);
        return {
          ok: true,
          headers: new Headers(),
          body: new ReadableStream({
            pull(ctrl) {
              pulls++;
              if (pulls <= 10) ctrl.enqueue(chunk);
              else ctrl.close();
            },
          }, { highWaterMark: 0 }),
        };
      };
      const r3 = await bc.copied({ gif: 'https://example.com/stream-huge.gif' });
      assert.equal(r3, null);
      assert.equal(aborted, true);
      assert.ok(pulls <= 4, `expected stream abort at <=4 chunks, got ${pulls}`);
      assert.equal(bc.list().length, 0);
      assert.equal(fs.readdirSync(clipDir).filter((f) => f.endsWith('.gif')).length, 0);
    });
  });

  it('deduplicates images by PNG sha256 hash and awaits clipboard:suppress before writeImage (M34)', async () => {
    await withBetterClipboardMocks(async ({ bc, state, clipDir }) => {
      const fakePng = Buffer.from([0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4]);
      state.clipboardImage = {
        isEmpty: () => false,
        getSize: () => ({ width: 100 }),
        resize() { return this; },
        toPNG: () => fakePng,
      };
      const fakeCore = {
        async ui(ch, arg) {
          state.callLog.push(`core:${ch}:${JSON.stringify(arg)}`);
          return true;
        },
      };
      await bc.enable(fakeCore, false);
      bc.copied({ image: true });
      bc.copied({ image: true });

      const items = bc.list();
      assert.equal(items.length, 1);
      assert.equal(items[0].kind, 'image');
      assert.equal(typeof items[0].hash, 'string');
      assert.equal(items[0].hash.length, 16);
      assert.equal(fs.readdirSync(clipDir).filter((f) => f.endsWith('.png')).length, 1);

      state.callLog.length = 0;
      await bc.pick(items[0].id);
      assert.deepEqual(state.callLog, [
        'core:clipboard:suppress:800',
        'clipboard:writeImage',
        'core:clipboard:paste_current:{"hwnd":0}',
      ]);
    });
  });

  it('restores OS clipboard history synchronously on quit and blocks in-flight enable() (M31)', async () => {
    Object.defineProperty(process, 'platform', { value: 'win32', configurable: true });
    await withBetterClipboardMocks(async ({ bc, state }) => {
      state.regStore['HKCU\\Software\\Microsoft\\Clipboard\\EnableClipboardHistory'] = 1;
      let releaseEnableUi;
      const fakeCore = {
        ui: () => new Promise((res) => { releaseEnableUi = res; }),
      };

      const pendingEnable = bc.enable(fakeCore, true);
      state.regStore['HKCU\\Software\\Murmint\\ClipboardHistoryWas'] = 1;
      state.regStore['HKCU\\Software\\Microsoft\\Clipboard\\EnableClipboardHistory'] = 0;

      assert.equal(bc.restoreOsHistorySync(), true);
      assert.equal(state.regStore['HKCU\\Software\\Microsoft\\Clipboard\\EnableClipboardHistory'], 1);
      assert.equal(state.regStore['HKCU\\Software\\Murmint\\ClipboardHistoryWas'], undefined);

      releaseEnableUi(true);
      await pendingEnable;
      assert.equal(state.regStore['HKCU\\Software\\Microsoft\\Clipboard\\EnableClipboardHistory'], 1);
    });
  });
});
