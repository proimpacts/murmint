'use strict';
const { spawn } = require('child_process');
const EventEmitter = require('events');
const fs = require('fs');
const path = require('path');

const EXE = process.platform === 'win32' ? 'murmint-engine.exe' : 'murmint-engine';

function locate() {
  const candidates = [
    process.env.MURMINT_ENGINE,
    process.resourcesPath && path.join(process.resourcesPath, 'bin', EXE),
    path.join(__dirname, '..', '..', 'bin', EXE),
    path.join(__dirname, '..', '..', 'engine', 'target', 'release', EXE),
    path.join(__dirname, '..', '..', 'engine', 'target', 'debug', EXE),
  ].filter(Boolean);
  return candidates.find((p) => { try { return fs.statSync(p).isFile(); } catch { return false; } }) || null;
}

class Core extends EventEmitter {
  constructor({ exePath = locate(), log = () => {} } = {}) {
    super();
    this.exePath = exePath; this.log = log;
    this.proc = null; this.pending = new Map(); this.seq = 0; this.buf = '';
    this.initArgs = null; this.quitting = false; this.restarts = 0;
  }

  get available() { return !!this.exePath; }

  start() {
    if (!this.exePath || this.proc) return false;
    const p = this.proc = spawn(this.exePath, [], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    p.stdout.setEncoding('utf8');
    p.stdout.on('data', (d) => { if (this.proc === p) this._data(d); });
    p.stderr.on('data', (d) => this.log(String(d).trim()));
    p.stdin.on('error', () => {});
    p.on('error', (e) => { this.log('core failed to start: ' + e.message); this._down(p); });
    p.on('exit', () => this._down(p));
    return true;
  }

  _down(p) {
    if (this.proc !== p) return;
    this.proc = null; this.buf = '';
    for (const { reject } of this.pending.values()) reject(new Error('the core stopped'));
    this.pending.clear();
    this.emit('down');
    if (!this.quitting && this.restarts < 5) {
      this.restarts++;
      setTimeout(() => {
        if (this.quitting || !this.initArgs || !this.start()) return;
        const p = this.proc;
        this.call('init', { ...this.initArgs, locked: true }, 8000).then((r) => {
          if (this.proc !== p) return;
          if (r.ok) this.emit('restarted');
          else p.kill();
        }).catch(() => { if (this.proc === p) p.kill(); });
      }, 600 * this.restarts);
    }
  }

  _data(chunk) {
    this.buf += chunk;
    let i;
    while ((i = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, i).trim(); this.buf = this.buf.slice(i + 1);
      if (!line) continue;
      let m; try { m = JSON.parse(line); } catch { continue; }
      if (m.ev === 'push') { this.emit('push', m); continue; }
      if (m.id != null) { const w = this.pending.get(m.id); if (w) { this.pending.delete(m.id); w.resolve(m); } }
    }
  }

  call(op, payload = {}, timeoutMs = 30000) {
    return new Promise((resolve, reject) => {
      if (!this.proc) return reject(new Error('the core is not running'));
      const id = ++this.seq;
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('the core did not answer')); }, timeoutMs);
      this.pending.set(id, { resolve: (m) => { clearTimeout(timer); resolve(m); }, reject: (e) => { clearTimeout(timer); reject(e); } });
      try { this.proc.stdin.write(JSON.stringify({ id, op, ...payload }) + '\n'); } catch (e) { clearTimeout(timer); this.pending.delete(id); reject(e); }
    });
  }

  init(dir, version, locked) { this.initArgs = { dir, version, locked }; return this.call('init', this.initArgs, 10000); }

  // dictation stays off until the person signs in and finishes onboarding; a restarted engine keeps the setting
  lock(locked) { if (this.initArgs) this.initArgs.locked = locked; return this.ui('session:lock', locked); }

  async ui(ch, ...args) { const r = await this.call('ui', { ch, args }, 60000); return r.value; }

  post(op, payload = {}) { try { this.proc && this.proc.stdin.write(JSON.stringify({ op, ...payload }) + '\n'); } catch { } }

  send(ch, ...args) { try { this.proc && this.proc.stdin.write(JSON.stringify({ op: 'send', ch, args }) + '\n'); } catch { } }

  stop() {
    this.quitting = true;
    try { this.proc && this.proc.stdin.write(JSON.stringify({ id: ++this.seq, op: 'quit' }) + '\n'); this.proc && this.proc.stdin.end(); } catch { }
    const p = this.proc;
    setTimeout(() => { try { p && p.kill(); } catch { } }, 600);
  }
}

module.exports = { Core, locate, EXE };
