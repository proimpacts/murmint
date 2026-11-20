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

  let test = null;
  async function micTest(onOff, device) {
    if (test) { cancelAnimationFrame(test.raf); test.stream.getTracks().forEach((t) => t.stop()); test.ac.close(); test = null; emitLocal('mic:level', 0); }
    if (!onOff) return;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: device && device !== 'default' ? { deviceId: { exact: device } } : true });
      const ac = new AudioContext(); const an = ac.createAnalyser(); an.fftSize = 512; ac.createMediaStreamSource(stream).connect(an);
      const buf = new Float32Array(an.fftSize);
      test = { stream, ac, raf: 0 };
      const tick = () => { an.getFloatTimeDomainData(buf); let s = 0; for (const v of buf) s += v * v; emitLocal('mic:level', Math.min(1, Math.sqrt(s / buf.length) * 9)); test.raf = requestAnimationFrame(tick); };
      tick();
    } catch { }
  }

  window.murmint = {
    ...ipc,
    on,
    audioDevices,
    micTest,
    win: { minimize: () => ipc.winAction('min'), toggleMaximize: () => ipc.winAction('max'), close: () => ipc.winAction('close') },
  };
})();
