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
  const ICONS = {
    Smileys: '<circle cx="10" cy="10" r="7.5"/><path d="M7 12.2c.8 1 1.8 1.5 3 1.5s2.2-.5 3-1.5"/><circle class="dot" cx="7.6" cy="8.2" r=".9"/><circle class="dot" cx="12.4" cy="8.2" r=".9"/>',
    People: '<circle cx="10" cy="6.5" r="3"/><path d="M4 17c.6-3.2 3-5 6-5s5.4 1.8 6 5"/>',
    Nature: '<path d="M15.5 4.5C9 4.5 5 8 5 13c0 1 .2 1.8.5 2.5M15.5 4.5c0 6.5-3.5 10.5-8.5 10.5M5.5 15.5 11 10"/>',
    Food: '<path d="M4 9h12a6 6 0 0 1-12 0zM7 5.5c0-1 1-1 1-2M10 5.5c0-1 1-1 1-2M13 5.5c0-1 1-1 1-2"/>',
    Travel: '<path d="M3 11.5 17 6l-3.5 10-3-4.5zM10.5 11.5 17 6"/>',
    Activities: '<circle cx="10" cy="10" r="7.5"/><path d="M10 6.5 13 8.7l-1.2 3.6H8.2L7 8.7zM10 2.5v4M13 8.7l4-1.2M11.8 12.3l2.4 3.4M8.2 12.3l-2.4 3.4M7 8.7 3 7.5"/>',
    Objects: '<path d="M7.5 14.5h5M8 17h4M10 2.5a5 5 0 0 0-3 9c.6.5 1 1.3 1 2h4c0-.7.4-1.5 1-2a5 5 0 0 0-3-9z"/>',
    Symbols: '<path d="M10 16.5s-6.5-3.8-6.5-8.3A3.5 3.5 0 0 1 10 6.3a3.5 3.5 0 0 1 6.5 1.9c0 4.5-6.5 8.3-6.5 8.3z"/>',
    Flags: '<path d="M5 17.5V3M5 3.5h9.5l-2 3.5 2 3.5H5"/>',
  };
  let items = [], tab = 'clips', group = EMOJI[0][0], hot = 0, picks = [];
  const recent = () => { try { return JSON.parse(localStorage.getItem('recentEmoji') || '[]'); } catch { return []; } };
  const remember = (e) => { try { localStorage.setItem('recentEmoji', JSON.stringify([e, ...recent().filter((x) => x !== e)].slice(0, 24))); } catch { } };

  const pick = (what) => {
    if (what.emoji) remember(what.emoji);
    ipc.clipboardPick(what.id || what);
  };
  const src = (x) => (x.file ? 'file:///' + String(x.file).replace(/\\/g, '/') : x.url);

  const fileLabel = (x) => {
    const list = Array.isArray(x.files) && x.files.length ? x.files : String(x.text || '').split(/\r?\n/).filter(Boolean);
    const names = list.map((p) => p.split(/[\\/]/).pop() || p);
    if (names.length <= 1) return names[0] || '';
    if (names.length <= 3) return `${names.length} files: ${names.join(', ')}`;
    return `${names.length} files: ${names.slice(0, 2).join(', ')} (+${names.length - 2} more)`;
  };

  function clipRow(x) {
    let body;
    if (x.kind === 'image' || x.kind === 'gif') body = el('img', { src: src(x), alt: '', loading: 'lazy' });
    else if (x.kind === 'files') body = el('div', { className: 'files txt', textContent: fileLabel(x), title: x.text || '' });
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
        const c = el('button', { className: g === group ? 'on' : '', title: g, 'aria-label': g });
        c.innerHTML = `<svg viewBox="0 0 20 20">${ICONS[g] || ''}</svg>`;
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
