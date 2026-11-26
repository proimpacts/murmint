// Better Clipboard: what you copied and emojis. A pick is pasted into the window you were in.
(() => {
  const ipc = window.murmintIpc;
  const $ = (id) => document.getElementById(id);
  const el = (tag, props = {}, ...kids) => { const n = Object.assign(document.createElement(tag), props); n.append(...kids.filter((k) => k != null)); return n; };
  const ICON = {
    pin: '<svg viewBox="0 0 16 16"><path d="M9.5 2.5l4 4-2 1-2.5 2.5.5 3-1 1-2.5-2.5L3 14.5 1.5 13l3-3L2 7.5l1-1 3 .5L8.5 4.5z"/></svg>',
    del: '<svg viewBox="0 0 16 16"><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 8.5h5.6l.7-8.5"/></svg>',
  };
  const seg = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
  const EMOJI = window.EMOJI.map(([group, chars, names]) => {
    const list = [...seg.segment(chars)].map((s) => s.segment);
    const n = names.split('|');
    return [group, list.map((e, i) => [e, n[i] || ''])];
  });

  // one emoji stands for each group, so every group fits in a single row
  const ICONS = { Smileys: '😀', People: '👋', Nature: '🐶', Food: '🍔', Travel: '✈️', Activities: '⚽', Objects: '💡', Symbols: '❤️', Flags: '🏁' };
  let items = [], tab = 'clips', group = EMOJI[0][0], hot = 0, picks = [];
  const recent = () => { try { return JSON.parse(localStorage.getItem('recentEmoji') || '[]'); } catch { return []; } };
  const remember = (e) => { try { localStorage.setItem('recentEmoji', JSON.stringify([e, ...recent().filter((x) => x !== e)].slice(0, 24))); } catch { } };

  const pick = (what) => {
    if (what.emoji) remember(what.emoji);
    ipc.clipboardPick(what.id || what);
  };
  const src = (x) => (x.file ? 'file:///' + String(x.file).replace(/\\/g, '/') : x.url);

  function clipRow(x) {
    let body;
    if (x.kind === 'image' || x.kind === 'gif') body = el('img', { src: src(x), alt: '', loading: 'lazy' });
    else if (x.kind === 'files') body = el('div', { className: 'files txt', textContent: x.text });
    else body = el('div', { className: 'txt', textContent: x.text });
    const acts = el('div', { className: 'acts' });
    const act = (name, title, fn) => { const b = el('button', { title, innerHTML: ICON[name] }); b.addEventListener('click', (e) => { e.stopPropagation(); fn(); }); return b; };
    acts.append(act('pin', x.pinned ? 'Unpin' : 'Pin', () => ipc.clipboardPin(x.id)), act('del', 'Delete', () => ipc.clipboardRemove(x.id)));
    const row = el('button', { className: 'clip' }, body, acts, x.pinned ? el('span', { className: 'pin-on', innerHTML: ICON.pin }) : null);
    row.addEventListener('click', () => pick(x));
    return row;
  }

  function draw() {
    const q = $('search').value.trim().toLowerCase();
    const body = $('body'), foot = $('foot'), chips = $('chips');
    body.replaceChildren(); foot.replaceChildren(); chips.replaceChildren();
    chips.classList.toggle('on', tab === 'emoji' && !q);
    picks = [];
    if (tab === 'clips') {
      const list = items.filter((x) => !q || (x.text || x.url || x.source || '').toLowerCase().includes(q) || (q === 'gif' && x.kind === 'gif'));
      list.sort((a, b) => (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0));
      if (!list.length) body.append(el('div', { className: 'cb-empty' }, el('div', {}, el('b', { textContent: q ? 'Nothing matches' : 'Nothing copied yet' }), q ? '' : 'Copy something and it shows up here. What Murmint types for you never does.')));
      for (const x of list) { const r = clipRow(x); picks.push(r); body.append(r); }
      const clear = items.some((x) => !x.pinned) ? el('button', { textContent: 'Clear all', title: 'Pinned items stay' }) : null;
      if (clear) clear.addEventListener('click', () => ipc.clipboardClear());
      if (!q) foot.append(...[el('b', { textContent: 'Clipboard history' }), clear].filter(Boolean));
    } else if (tab === 'emoji') {
      const sections = q
        ? [['Results', EMOJI.flatMap(([, l]) => l).filter(([, n]) => n.includes(q)).slice(0, 160)]]
        : [...(recent().length ? [['Recent', recent().map((e) => [e, ''])]] : []), ...EMOJI.filter(([g]) => g === group)];
      if (!q) for (const [g] of EMOJI) {
        const c = el('button', { className: g === group ? 'on' : '', textContent: ICONS[g] || g, title: g, 'aria-label': g });
        c.addEventListener('click', () => { group = g; hot = 0; draw(); body.scrollTop = 0; });
        chips.append(c);
      }
      for (const [g, list] of sections) {
        if (!list.length) continue;
        const grid = el('div', { className: 'emojis' });
        for (const [e, name] of list) {
          const b = el('button', { textContent: e, title: name });
          b.addEventListener('click', () => pick({ emoji: e }));
          picks.push(b); grid.append(b);
        }
        body.append(el('div', { className: 'group', textContent: g }), grid);
      }
      if (q && !sections[0][1].length) body.append(el('div', { className: 'cb-empty' }, el('div', {}, el('b', { textContent: 'No emoji found' }))));
    }
    hot = Math.min(hot, Math.max(0, picks.length - 1));
    mark();
  }
  const mark = () => picks.forEach((p, i) => { p.classList.toggle('hot', i === hot); if (i === hot) p.scrollIntoView({ block: 'nearest' }); });

  function setTab(t) {
    tab = t; hot = 0;
    document.querySelectorAll('.cb-tabs button').forEach((b) => b.setAttribute('aria-selected', String(b.dataset.tab === t)));
    draw();
  }
  document.querySelectorAll('.cb-tabs button').forEach((b) => b.addEventListener('click', () => setTab(b.dataset.tab)));
  $('close').addEventListener('click', () => ipc.clipboardClose());
  // dragged by its top bar; the main process moves the window with the mouse until the button is let go
  $('grip').addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || e.target.closest('button')) return;
    $('grip').setPointerCapture(e.pointerId);
    ipc.clipboardDrag(true);
  });
  const drop = () => ipc.clipboardDrag(false);
  $('grip').addEventListener('pointerup', drop);
  $('grip').addEventListener('lostpointercapture', drop);
  $('search').addEventListener('input', () => { hot = 0; draw(); });
  document.addEventListener('keydown', (e) => {
    const cols = tab === 'emoji' ? 8 : 1;
    if (e.key === 'Escape') { ipc.clipboardClose(); return; }
    if (e.key === 'Tab') { e.preventDefault(); setTab(tab === 'clips' ? 'emoji' : 'clips'); return; }
    if (e.key === 'ArrowDown') { e.preventDefault(); hot = Math.min(picks.length - 1, hot + cols); mark(); return; }
    if (e.key === 'ArrowUp') { e.preventDefault(); hot = Math.max(0, hot - cols); mark(); return; }
    if (e.key === 'ArrowRight' && cols > 1) { e.preventDefault(); hot = Math.min(picks.length - 1, hot + 1); mark(); return; }
    if (e.key === 'ArrowLeft' && cols > 1) { e.preventDefault(); hot = Math.max(0, hot - 1); mark(); return; }
    if (e.key === 'Enter' && picks[hot]) { e.preventDefault(); picks[hot].click(); return; }
    if (e.key.length === 1 && document.activeElement !== $('search')) $('search').focus();
  });

  async function theme() {
    try {
      const st = await ipc.getState();
      const a = st.config.appearance;
      const dark = a.theme === 'dark' || (a.theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
      document.documentElement.dataset.theme = dark ? 'dark' : 'light';
      document.documentElement.dataset.accent = a.accent || 'ember';
    } catch { }
  }
  ipc.on('clipboard:items', (list) => { items = list || []; draw(); });
  ipc.on('clipboard:shown', (t) => { theme(); $('search').value = ''; hot = 0; setTab(t === 'emoji' ? 'emoji' : 'clips'); $('search').focus(); });
  theme();
  ipc.clipboardItems().then((list) => { items = list || []; draw(); });
})();
