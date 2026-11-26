(() => {
  const api = window.murmint;
  const $ = (id) => document.getElementById(id);
  const stage = $('stage'), pill = $('pill'), bubble = $('bubble'), dotsEl = $('dots'), label = $('label');
  const btext = $('btext'), bacts = $('bacts'), bcopy = $('bcopy'), bdismiss = $('bdismiss'), appico = $('appico');
  const LABEL = { listening: 'Listening', paused: 'Paused', processing: 'Writing', success: 'Done', error: 'Try again' };
  const N = 5;
  const dots = Array.from({ length: N }, () => dotsEl.appendChild(document.createElement('i')));

  let state = 'idle', prevState = 'idle', sounds = true, lvl = 0, smooth = 0;

  let ac;
  function blip(freq, dur = 0.08, vol = 0.04) {
    if (!sounds) return;
    try {
      ac ||= new AudioContext();
      const o = ac.createOscillator(), g = ac.createGain();
      o.frequency.value = freq; o.type = 'sine';
      g.gain.setValueAtTime(0, ac.currentTime); g.gain.linearRampToValueAtTime(vol, ac.currentTime + 0.01); g.gain.exponentialRampToValueAtTime(0.0001, ac.currentTime + dur);
      o.connect(g).connect(ac.destination); o.start(); o.stop(ac.currentTime + dur + 0.02);
    } catch { }
  }

  const rec = new Recorder({
    onLevel: (rms) => { lvl = Math.min(1, rms * 9); },
    onSegment: (seg) => api.sendSegment(seg),
    onPreview: (p) => api.sendPreview(p),
    onCaption: () => {},
    onChunk: (buf) => api.sendChunk(buf),
    onBoundary: (b) => api.sendBoundary(b),
    onAutoStop: () => api.sendAutoStop(),
    onError: (m) => api.recError(m),
  });
  let chain = Promise.resolve();
  api.on('rec:cmd', ({ cmd, opts }) => {
    chain = chain.then(async () => {
      if (cmd === 'start') { sounds = opts.sounds; await rec.start(opts); }
      else if (cmd === 'stop') rec.stop();
      else if (cmd === 'pause') rec.pause();
      else if (cmd === 'resume') rec.resume();
      else if (cmd === 'cancel') rec.cancel();
    }).catch((e) => api.recError(String((e && e.message) || e)));
  });

  let hideTimer = 0, pinnedText = '';
  const DONE_MS = 1200;
  const PINNED_MS = 10 * 60 * 1000;
  function showFor(msg, ms, copyable = false) {
    clearTimeout(hideTimer); hideTimer = 0;
    say(msg, copyable);
    if (msg) hideTimer = setTimeout(() => { hideTimer = 0; say(''); }, ms);
  }
  const MAX_WORDS = 12;
  const short = (t) => { const w = String(t).trim().split(/\s+/); return w.length > MAX_WORDS ? w.slice(0, MAX_WORDS).join(' ') + '…' : String(t).trim(); };
  function say(msg, copyable = false) {
    const wasPinned = !!pinnedText;
    pinnedText = msg && copyable ? msg : '';
    bacts.hidden = !pinnedText;
    bubble.classList.toggle('pinned', !!pinnedText);
    if (wasPinned !== !!pinnedText) api.overlayPin(!!pinnedText);
    if (!msg) { bubble.classList.remove('on'); api.overlayHover(false); return; }
    btext.textContent = short(msg); bubble.classList.add('on');
    btext.scrollTop = btext.scrollHeight;
    btext.classList.toggle('long', btext.scrollHeight > btext.clientHeight + 1);
  }
  bcopy.addEventListener('click', async () => {
    if (!pinnedText) return;
    try { await api.copyText(pinnedText); bcopy.textContent = 'Copied'; } catch { bcopy.textContent = 'Could not copy'; }
    setTimeout(() => { bcopy.textContent = 'Copy'; showFor('', 0); }, 700);
  });
  bdismiss.addEventListener('click', () => showFor('', 0));

  let iconFor = '';
  async function showAppIcon(exe, site) {
    const key = site || exe;
    if (!key) { appico.hidden = true; return; }
    if (key === iconFor && !appico.hidden) return;
    iconFor = key;
    let url = site ? await api.siteIcon(site).catch(() => null) : null;
    if (!url && exe) url = await api.appIcon(exe).catch(() => null);
    if (iconFor !== key) return;
    if (!url) { appico.hidden = true; return; }
    const shape = await iconShape(url).catch(() => null);
    if (iconFor !== key) return;
    appico.classList.toggle('round', !!(shape && shape.round));
    appico.firstElementChild.src = shape && shape.round ? shape.cropped : url;
    appico.hidden = false;
  }

  function iconShape(url) {
    return new Promise((resolve, reject) => {
      const im = new Image();
      im.onload = () => {
        const N = 64, c = document.createElement('canvas'); c.width = c.height = N;
        const g = c.getContext('2d'); g.drawImage(im, 0, 0, N, N);
        const a = g.getImageData(0, 0, N, N).data, at = (x, y) => a[(Math.round(y) * N + Math.round(x)) * 4 + 3];
        let x0 = N, y0 = N, x1 = -1, y1 = -1;
        for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) if (at(x, y) > 40) { if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y; }
        const w = x1 - x0 + 1, h = y1 - y0 + 1;
        if (x1 < 0 || Math.abs(w - h) > 3) return resolve({ round: false });
        const cx = x0 + (w - 1) / 2, cy = y0 + (h - 1) / 2, r = (w - 1) / 2;
        let ring = true;
        for (let i = 0; i < 24; i++) { const t = (i / 24) * Math.PI * 2; if (at(cx + Math.cos(t) * r * 0.86, cy + Math.sin(t) * r * 0.86) < 200) { ring = false; break; } }
        const k = r * 0.12;
        const corners = [[x0 + k, y0 + k], [x1 - k, y0 + k], [x0 + k, y1 - k], [x1 - k, y1 - k]].every(([x, y]) => at(x, y) < 60);
        if (!(ring && corners)) return resolve({ round: false });
        const out = document.createElement('canvas'); out.width = out.height = 64;
        out.getContext('2d').drawImage(im, (x0 / N) * im.naturalWidth, (y0 / N) * im.naturalHeight, (w / N) * im.naturalWidth, (h / N) * im.naturalHeight, 0, 0, 64, 64);
        resolve({ round: true, cropped: out.toDataURL('image/png') });
      };
      im.onerror = reject;
      im.src = url;
    });
  }

  let poofTimer = 0, foldTimer = 0, folding = false;
  function render(s) {
    state = s.state;
    stage.dataset.state = state === 'idle' ? 'idle' : 'on';
    const attached = document.documentElement.dataset.pill === 'attached';
    pill.dataset.state = state;
    // attached, starting to talk: the pill poofs out of the edge of the screen
    if (attached && state === 'listening' && prevState === 'idle') {
      pill.classList.remove('poof');
      void pill.offsetWidth;
      pill.classList.add('poof');
      clearTimeout(poofTimer);
      poofTimer = setTimeout(() => pill.classList.remove('poof'), 600);
    }
    // attached, finished: the pill shrinks slowly back to its small size, still joined to the edge; its words fold
    // away as it goes, rather than vanishing first
    if (attached && state === 'idle' && prevState !== 'idle' && label.textContent) {
      clearTimeout(foldTimer);
      folding = true;
      label.style.maxWidth = label.scrollWidth + 'px'; // start from its real width, so it shrinks evenly
      void label.offsetWidth;
      label.classList.add('fold');
      foldTimer = setTimeout(() => { folding = false; if (state === 'idle') label.textContent = ''; label.classList.remove('fold'); label.style.maxWidth = ''; }, 520);
    } else if (!(folding && state === 'idle')) {
      // a repeated "idle" while it is still folding must not cut the fold short
      clearTimeout(foldTimer);
      folding = false;
      label.classList.remove('fold');
      label.style.maxWidth = '';
      label.textContent = s.label || LABEL[state] || '';
    }
    if (state === 'listening' || state === 'paused' || state === 'processing') { clearTimeout(hideTimer); hideTimer = 0; say(''); }
    if (s.exe || s.site) showAppIcon(s.exe, s.site);
    if (state === 'idle') appico.hidden = true;
    if (state === 'listening' && s.starting) blip(660);
    if (state === 'processing' && prevState === 'listening') blip(440);
    if (state === 'success') {
      if (s.copy && s.text) showFor(s.text, PINNED_MS, true);
      else showFor(s.text || s.message || '', s.text ? DONE_MS : 2500);
      blip(880, 0.1, 0.035);
    }
    if (state === 'error') { showFor(s.message || 'Something went wrong', 3800); blip(220, 0.14, 0.05); }
    if (state === 'idle' && !hideTimer) say('');
    prevState = state;
    if (state === 'listening' || state === 'processing') ensureLoop();
  }
  api.on('overlay:state', render);

  let dragging = false;
  pill.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    dragging = true; stage.classList.add('dragging');
    api.overlayDrag('start', { sx: e.screenX, sy: e.screenY });
    e.preventDefault();
  });
  window.addEventListener('mousemove', (e) => { if (dragging) api.overlayDrag('move', { sx: e.screenX, sy: e.screenY }); });
  window.addEventListener('mouseup', (e) => { if (!dragging) return; dragging = false; stage.classList.remove('dragging'); api.overlayDrag('end', { sx: e.screenX, sy: e.screenY }); });

  for (const el of [pill, bubble]) { el.addEventListener('mouseenter', () => api.overlayHover(true)); el.addEventListener('mouseleave', () => api.overlayHover(false)); }

  const applyTheme = (st) => {
    const a = st.config.appearance;
    document.documentElement.dataset.theme = a.theme === 'system' ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : a.theme;
    document.documentElement.dataset.accent = a.accent;
    document.documentElement.dataset.motion = a.reduceMotion ? 'reduced' : '';
    sounds = a.sounds;
    const side = ['left', 'right', 'top', 'bottom'].includes(a.pillSide) ? a.pillSide : 'right';
    const look = a.pillStyle === 'attached' ? 'attached' : 'floating';
    const moved = stage.dataset.side !== side || document.documentElement.dataset.pill !== look;
    document.documentElement.dataset.pill = look;
    stage.dataset.side = side;
    if (moved && popReady && !dropping) pop('in');
    popReady = true;
  };
  // a drop: shrink away, let the window jump to the middle of the edge, then pop back in there
  let popReady = false, popTimer = 0, dropping = false;
  function pop(phase) {
    clearTimeout(popTimer);
    pill.classList.remove('pop-in', 'pop-out');
    void pill.offsetWidth;
    pill.classList.add(phase === 'out' ? 'pop-out' : 'pop-in');
    dropping = phase === 'out';
    if (phase === 'out') popTimer = setTimeout(() => pop('in'), 130);
    else popTimer = setTimeout(() => pill.classList.remove('pop-in'), 520);
  }
  api.on('overlay:pop', () => pop('out'));
  api.getState().then(applyTheme);
  api.on('state:changed', applyTheme);

  let running = false;
  const ensureLoop = () => { if (!running) { running = true; requestAnimationFrame(frame); } };
  function frame(t) {
    if (state !== 'listening' && state !== 'processing') {
      dots.forEach((d) => { d.style.removeProperty('--s'); d.style.opacity = ''; });
      running = false; return;
    }
    smooth += (lvl - smooth) * 0.3;
    if (state === 'listening') {
      for (let i = 0; i < N; i++) {
        const wave = 0.5 + 0.5 * Math.sin(t * 0.009 - i * 0.85);
        const k = Math.min(1, 0.12 + smooth * 1.25 * wave);
        dots[i].style.setProperty('--s', (0.75 + k * 0.9).toFixed(2));
        dots[i].style.opacity = (0.6 + k * 0.4).toFixed(2);
      }
    } else if (state === 'processing') {
      const head = (t / 140) % (N + 3);
      for (let i = 0; i < N; i++) {
        const d = Math.max(0, 1 - Math.abs(i - head) / 2.2);
        dots[i].style.setProperty('--s', (0.8 + d * 0.8).toFixed(2));
        dots[i].style.opacity = (0.25 + d * 0.75).toFixed(2);
      }
    }
    requestAnimationFrame(frame);
  }
})();
