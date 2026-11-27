(function () {
  const RATE = 16000;
  const BLOCK = 1024;
  const PREROLL_BLOCKS = 6;
  const WARM_MS = 30000;

  function encodeWav(samples) {
    const buf = new ArrayBuffer(44 + samples.length * 2);
    const v = new DataView(buf);
    const w = (o, s) => { for (let i = 0; i < s.length; i++) v.setUint8(o + i, s.charCodeAt(i)); };
    w(0, 'RIFF'); v.setUint32(4, 36 + samples.length * 2, true); w(8, 'WAVE'); w(12, 'fmt ');
    v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
    v.setUint32(24, RATE, true); v.setUint32(28, RATE * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
    w(36, 'data'); v.setUint32(40, samples.length * 2, true);
    for (let i = 0; i < samples.length; i++) { const s = Math.max(-1, Math.min(1, samples[i])); v.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true); }
    return buf;
  }
  const concat = (chunks) => { const out = new Float32Array(chunks.length * BLOCK); chunks.forEach((c, i) => out.set(c, i * BLOCK)); return out; };
  const frameRms = (a, from, to) => { let s = 0; for (let i = from; i < to; i++) s += a[i] * a[i]; return Math.sqrt(s / Math.max(1, to - from)); };

  function trimSilence(a, thr) {
    const F = 160; let first = -1, last = -1;
    for (let i = 0; i * F < a.length; i++) { if (frameRms(a, i * F, Math.min(a.length, (i + 1) * F)) > thr) { if (first < 0) first = i; last = i; } }
    if (first < 0) return a;
    return a.subarray(Math.max(0, first * F - Math.floor(RATE / 5)), Math.min(a.length, (last + 1) * F + Math.floor(RATE * 0.3)));
  }

  class Recorder {
    constructor(h) {
      this.h = h;
      this.stream = null; this.ctx = null; this.node = null; this.deviceKey = null;
      this.recording = false; this.paused = false; this.opts = {};
      this.warmTimer = null; this.previewTimer = null; this.previewBusy = false;
      this.ring = [];
      this._resetSeg();
      this.noiseFloor = 0.004;
    }

    now() { return typeof this.clock === 'function' ? this.clock() : performance.now(); }

    _pcm(f) {
      const pcm = new Int16Array(f.length);
      for (let i = 0; i < f.length; i++) { const v = Math.max(-1, Math.min(1, f[i])); pcm[i] = v < 0 ? v * 0x8000 : v * 0x7fff; }
      return pcm.buffer;
    }

    _resetSeg() { this.seg = { chunks: [], samples: 0, peak: 0, hadSpeech: false, silentRun: 0, lead: null }; }

    async _ensure(deviceId) {
      clearTimeout(this.warmTimer);
      const key = deviceId || 'default';
      if (this.stream && this.deviceKey === key && this.stream.getAudioTracks().every((t) => t.readyState === 'live')) return;
      this._releaseMic();
      const audio = { channelCount: 1, echoCancellation: false, noiseSuppression: true, autoGainControl: true };
      if (deviceId && deviceId !== 'default') audio.deviceId = { exact: deviceId };
      try { this.stream = await navigator.mediaDevices.getUserMedia({ audio }); }
      catch (e) { if (audio.deviceId) { delete audio.deviceId; this.stream = await navigator.mediaDevices.getUserMedia({ audio }); } else throw e; }
      this.deviceKey = key;
      // the audio context and capture worklet are built once and kept, so reopening the mic is all a cold start costs
      if (!this.ctx || this.ctx.state === 'closed') {
        this.ctx = new AudioContext({ sampleRate: RATE, latencyHint: 'interactive' });
        await this.ctx.audioWorklet.addModule(this.opts.workletUrl || 'recorder-worklet.js');
        this.node = new AudioWorkletNode(this.ctx, 'capture');
        this.node.port.onmessage = (e) => this._block(e.data);
      }
      this.src = this.ctx.createMediaStreamSource(this.stream);
      this.src.connect(this.node);
    }

    // lets go of the microphone (so Windows stops showing it in use) but keeps the audio pipeline ready
    _releaseMic() {
      try { this.src?.disconnect(); } catch { }
      try { this.stream?.getTracks().forEach((t) => t.stop()); } catch { }
      this.stream = this.src = null; this.deviceKey = null;
      try { if (this.ctx && this.ctx.state === 'running') this.ctx.suspend(); } catch { }
    }

    _teardown() {
      this._releaseMic();
      try { this.ctx?.close(); } catch { }
      this.ctx = this.node = null;
    }

    async start(opts) {
      this.opts = opts; this._resetSeg(); this.paused = false;
      this.lastSpeechAt = null; this.autoStopped = false; this.prevTrail = null;
      try { await this._ensure(opts.deviceId); } catch (e) {
        this.h.onError(e.name === 'NotAllowedError' ? 'Microphone access is blocked' : e.name === 'NotFoundError' ? 'No microphone found' : `Microphone error: ${e.message}`);
        return false;
      }
      if (this.ctx.state === 'suspended') await this.ctx.resume();
      this.recording = true; this.t0 = performance.now();
      const pre = this.ring; this.ring = [];
      for (const f of pre) { if (opts.stream) this.h.onChunk(this._pcm(f)); else this.seg.chunks.push(f); this.seg.samples += f.length; }
      this.lastSpeechAt = this.t0 + 3000;
      clearInterval(this.previewTimer);
      if (opts.preview && !opts.stream) this.previewTimer = setInterval(() => this._preview(), 3000);
      return true;
    }

    _block(f) {
      let sum = 0, peak = 0;
      for (let i = 0; i < f.length; i++) { const a = Math.abs(f[i]); sum += f[i] * f[i]; if (a > peak) peak = a; }
      const rms = Math.sqrt(sum / f.length);
      if (!this.recording) {
        this.noiseFloor = this.noiseFloor * 0.98 + rms * 0.02; this.h.onLevel(0);
        this.ring.push(f.slice()); if (this.ring.length > PREROLL_BLOCKS) this.ring.shift();
        return;
      }
      this.h.onLevel(this.paused ? 0 : rms);
      if (this.paused) return;
      const s = this.seg;
      if (this.opts.stream) {
        this.h.onChunk(this._pcm(f));
      } else s.chunks.push(f);
      s.samples += f.length; if (peak > s.peak) s.peak = peak;
      const thr = Math.max(0.012, this.noiseFloor * 3.2);
      const inSpeech = s.hadSpeech && s.silentRun / RATE < 0.5;
      const loud = rms > thr || (inSpeech && rms > thr * 0.45);
      if (loud) { if (!s.hadSpeech) s.lead = s.samples - f.length; s.hadSpeech = true; s.silentRun = 0; this.lastSpeechAt = this.now(); } else { s.silentRun += f.length; this.noiseFloor = this.noiseFloor * 0.97 + rms * 0.03; }
      if (this.opts.autoStopMs && !this.autoStopped && this.now() - this.lastSpeechAt >= this.opts.autoStopMs) { this.autoStopped = true; if (this.h.onAutoStop) this.h.onAutoStop(); }
      if (this.opts.continuous) {
        const silentMs = (s.silentRun / RATE) * 1000, durMs = (s.samples / RATE) * 1000;
        if (s.hadSpeech && silentMs >= this.opts.silenceMs && durMs > 600) this._emit(false);
        else if (!s.hadSpeech && durMs > 4000) this._resetSeg();
        else if (durMs > 8000 && silentMs >= 180) this._emit(false);
        else if (durMs > 45000) this._emit(false);
      }
    }

    _emit(final) {
      const s = this.seg;
      this._resetSeg();
      if (!final && (!s.hadSpeech || s.samples < RATE * 0.3)) return;
      const gapMs = this.prevTrail == null ? null : Math.round(this.prevTrail + ((s.lead || 0) / RATE) * 1000);
      this.prevTrail = (s.silentRun / RATE) * 1000;
      if (this.opts.stream) { this.h.onBoundary(this.opts.captureId, { final, peak: s.peak, gapMs }); return; }
      const trimmed = trimSilence(concat(s.chunks), Math.max(0.010, this.noiseFloor * 2.5));
      this.h.onSegment(this.opts.captureId, { wav: encodeWav(trimmed), durationSec: trimmed.length / RATE, peak: s.peak, final, gapMs });
    }

    async _preview(generation) {
      const s = this.seg;
      if (!this.recording || this.paused || this.previewBusy || !s.hadSpeech || s.samples < RATE) return;
      this.previewBusy = true;
      try {
        const chunks = s.chunks.slice(-Math.ceil((8 * RATE) / BLOCK));
        const text = await this.h.onPreview(this.opts.captureId, { wav: encodeWav(concat(chunks)), durationSec: (chunks.length * BLOCK) / RATE });
        if (generation === this.generation && this.recording && !this.paused && text) this.h.onCaption(text);
      } catch { /* previews can be dropped if the engine stops */ }
      finally { if (generation === this.generation) this.previewBusy = false; }
    }

    pause() { this.paused = true; }
    resume() { this.lastSpeechAt = this.now() + 3000; this.paused = false; }
    stop() { if (!this.recording) return; this.recording = false; this.ring = []; clearInterval(this.previewTimer); this._emit(true); this._scheduleRelease(); }
    cancel() {
      ++this.generation;
      this.recording = this.paused = false;
      clearInterval(this.previewTimer);
      clearTimeout(this.warmTimer);
      this._resetSeg();
      this._teardown();
    }
    _scheduleRelease() { clearTimeout(this.warmTimer); this.warmTimer = setTimeout(() => this._teardown(), WARM_MS); if (this.warmTimer && this.warmTimer.unref) this.warmTimer.unref(); }
  }

  window.Recorder = Recorder;
})();
