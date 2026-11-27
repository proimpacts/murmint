(function () {
  const ipc = window.murmintIpc;
  document.documentElement.classList.add('electron');

  const listeners = new Map();
  const on = (ch, fn) => {
    if (ch === 'mic:level') { const set = listeners.get(ch) || new Set(); set.add(fn); listeners.set(ch, set); return () => set.delete(fn); }
    return ipc.on(ch, fn);
  };
  const emitLocal = (ch, v) => { for (const fn of listeners.get(ch) || []) fn(v); };

  async function audioDevices() {
    let list = [];
    try {
      list = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audioinput');
      if (list.length && list.every((d) => !d.label)) {
        const s = await navigator.mediaDevices.getUserMedia({ audio: true });
        s.getTracks().forEach((t) => t.stop());
        list = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audioinput');
      }
    } catch { }
    const clean = (n) => (n || 'Microphone').replace(/^Default - /, '');
    const def = list.find((d) => d.deviceId === 'default');
    const devices = list.filter((d) => d.deviceId !== 'default' && d.deviceId !== 'communications').map((d) => ({ id: d.deviceId, name: clean(d.label), isDefault: false }));
    return { ok: list.length > 0, devices, defaultName: def ? clean(def.label) : '' };
  }

  let micGen = 0;
  let micActive = false;
  let test = null;
  const stopTest = () => {
    if (!test) return;
    const cur = test;
    test = null;
    if (cur.rafId) cancelAnimationFrame(cur.rafId);
    cur.stream?.getTracks().forEach((t) => t.stop());
    try { cur.ctx?.close?.(); } catch {}
  };
  async function micTest(onOff, device) {
    const myGen = ++micGen;
    micActive = !!onOff;
    stopTest();
    if (!onOff) {
      emitLocal('mic:level', 0);
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: device && device !== 'default' ? { deviceId: { exact: device } } : true });
      if (!micActive || myGen !== micGen) {
        stream.getTracks().forEach((t) => t.stop());
        return;
      }
      const ctx = new AudioContext();
      try {
        const an = ctx.createAnalyser();
        an.fftSize = 512;
        ctx.createMediaStreamSource(stream).connect(an);
        const buf = new Float32Array(an.fftSize);
        const cur = { stream, ctx, rafId: 0 };
        test = cur;
        const tick = () => {
          if (!micActive || myGen !== micGen || test !== cur) return;
          an.getFloatTimeDomainData(buf);
          let s = 0;
          for (const v of buf) s += v * v;
          emitLocal('mic:level', Math.min(1, Math.sqrt(s / buf.length) * 9));
          if (!micActive || myGen !== micGen || test !== cur) return;
          cur.rafId = requestAnimationFrame(tick);
        };
        tick();
      } catch {
        stream.getTracks().forEach((t) => t.stop());
        try { ctx.close?.(); } catch {}
        if (test && test.stream === stream) test = null;
      }
    } catch {
      if (myGen === micGen) micActive = false;
    }
  }

  window.murmint = {
    ...ipc,
    on,
    audioDevices,
    micTest,
    win: { minimize: () => ipc.winAction('min'), toggleMaximize: () => ipc.winAction('max'), close: () => ipc.winAction('close') },
  };
})();
