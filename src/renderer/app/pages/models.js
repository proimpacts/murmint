(function () {
  const { h, icon } = UI;
  const api = window.murmint;

  // ---- AI services: a service is a provider, its address and key, and the models chosen from it ----
  const DEFAULT_URL = { anthropic: 'https://api.anthropic.com/v1', openai: 'https://api.openai.com/v1' };
  const BUILTIN = [
    { key: 'anthropic', name: 'Anthropic (Claude)', docs: 'https://console.anthropic.com/settings/keys', keyLabel: 'Anthropic API key' },
    { key: 'openai', name: 'OpenAI API', docs: 'https://platform.openai.com/api-keys', keyLabel: 'OpenAI API key' },
    { key: 'cloudflare', name: 'Cloudflare Workers AI', docs: 'https://dash.cloudflare.com/profile/api-tokens', keyLabel: 'API token' },
  ];
  const eye = (open) => {
    const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('class', 'ic'); s.setAttribute('fill', 'none'); s.setAttribute('stroke', 'currentColor'); s.setAttribute('stroke-width', '1.8'); s.setAttribute('stroke-linecap', 'round');
    s.innerHTML = '<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>' + (open ? '' : '<path d="M4 4l16 16"/>');
    return s;
  };
  const tick = () => {
    const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('class', 'tick'); s.setAttribute('fill', 'none'); s.setAttribute('stroke', 'currentColor'); s.setAttribute('stroke-width', '2');
    s.innerHTML = '<circle cx="12" cy="12" r="9"/><path d="M8 12.5l2.8 2.8L16.2 9.6" stroke-linecap="round" stroke-linejoin="round"/>';
    return s;
  };
  const guessTask = (id) => (/whisper|transcri|voxtral|parakeet|\bstt\b/i.test(id) ? 'stt' : 'chat');

  async function serviceModal(existing) {
    const presets = await api.providerPresets();
    const removed = App.cfg().removedProviders || [];
    const choices = [...BUILTIN.filter((b) => !existing || b.key === existing.id), ...presets];
    let preset = existing
      ? choices.find((c) => c.key === (existing.builtin ? existing.id : existing.id.replace(/-[0-9a-f]{4}$/, ''))) || { key: existing.type === 'anthropic' ? 'custom-anthropic' : 'custom', type: existing.type, name: existing.name, keyLabel: existing.keyLabel, docs: existing.docs }
      : choices.find((c) => c.key === 'openrouter') || choices[0];
    const isBuiltin = () => BUILTIN.some((b) => b.key === preset.key);
    const isCustom = () => preset.key === 'custom' || preset.key === 'custom-anthropic';
    // the Claude API format: Anthropic itself and the services that copy it
    const isClaude = () => preset.key === 'anthropic' || preset.type === 'anthropic';
    const st = existing ? App.S.providerStatus[existing.id] || {} : {};

    const provider = UI.select(choices.map((c) => ({ value: c.key, label: c.key === 'custom' ? 'Other (OpenAI-compatible)' : c.key === 'custom-anthropic' ? 'Other (Anthropic-compatible)' : c.name })), preset.key,
      (v) => { preset = choices.find((c) => c.key === v); fill(); load(); }, 'w-full');
    if (existing) provider.disabled = true;
    const url = UI.input({ mono: true });
    const urlHint = h('div', { class: 'hint' });
    const field = (label, ...kids) => h('div', { class: 'svc-form', style: { gap: '6px' } }, h('label', null, label), ...kids);
    const acct = UI.input({ placeholder: '32-character Account ID', mono: true, value: existing?.accountId || '' });
    const acctWrap = field('Account ID', acct);
    const key = UI.input({ type: 'password', mono: true });
    const peek = h('button', { type: 'button', title: 'Show key', 'aria-label': 'Show key' }, eye(true));
    peek.addEventListener('click', () => { const show = key.type === 'password'; key.type = show ? 'text' : 'password'; peek.replaceChildren(eye(!show)); });
    const keyWrap = field('API Key', h('div', { class: 'keybox' }, key, peek));
    const consoleLink = h('a', { class: 'console', target: '_blank' });
    const name = UI.input({ placeholder: 'Name', value: existing && !existing.builtin ? existing.name : '' });
    const nameWrap = field('Name', name);

    // one selection menu per model; "Add another" adds a menu
    let options = [], known = new Map();
    const rows = h('div', { class: 'svc-form', style: { gap: '8px' } });
    const picked = () => [...rows.querySelectorAll('.select.dd')].map((s) => s.value).filter(Boolean);
    const menuOpts = () => (options.length ? options : [{ value: '-', label: 'Paste your key to load the models', disabled: true }]);
    const addRow = (value = '') => {
      const sel = UI.select([{ value: '', label: 'Choose a model' }, ...menuOpts()], value, () => {}, 'w-full');
      const row = h('div', { class: 'row2' }, sel,
        h('button', { class: 'btn icon ghost', type: 'button', title: 'Remove', on: { click: () => { row.remove(); if (!rows.children.length) addRow(); } } }, icon('x')));
      rows.append(row);
      return sel;
    };
    const redraw = () => { const keep = picked(); rows.replaceChildren(); (keep.length ? keep : ['']).forEach(addRow); };
    const more = UI.btn('Add another', { icon: 'plus', variant: 'sm', onClick: () => addRow().click() });
    const status = h('span', { class: 'note muted' });
    const say = (text, kind = 'muted') => { status.className = 'note ' + kind; status.replaceChildren(text); };

    function fill() {
      const local = !!preset.local;
      url.value = existing ? existing.baseUrl || '' : '';
      url.placeholder = preset.baseUrl || DEFAULT_URL[preset.key] || 'https://api.example.com/v1';
      urlHint.textContent = isCustom() ? `The address of any ${isClaude() ? 'Anthropic' : 'OpenAI'}-compatible server.` : 'Leave blank to use the official endpoint.';
      acctWrap.hidden = preset.key !== 'cloudflare';
      nameWrap.hidden = !isCustom();
      keyWrap.hidden = local && !!preset.keyOptional;
      key.value = '';
      key.placeholder = st.hasKey ? 'Saved. Paste a new key to replace it' : preset.keyOptional ? 'Optional' : 'Paste your API key';
      consoleLink.hidden = !preset.docs;
      consoleLink.href = preset.docs || '#';
      consoleLink.replaceChildren(local ? `Download ${preset.name}` : `Open the ${preset.name.replace(/\s*\(.*\)/, '')} console`, icon('external'));
      const have = existing ? existing.models : App.cfg().providers.find((p) => p.id === preset.key)?.models || [];
      known = new Map(have.map((m) => [m.id, m]));
      options = have.map((m) => ({ value: m.id, label: m.label }));
      rows.replaceChildren();
      (existing ? existing.models.map((m) => m.id) : ['']).forEach(addRow);
      say('');
    }

    const base = () => url.value.trim() || preset.baseUrl || DEFAULT_URL[preset.key] || existing?.baseUrl || '';
    let loadId = 0;
    async function load() {
      const k = key.value.trim();
      if (!base() || (!k && !preset.keyOptional && !existing)) return;
      const id = ++loadId;
      say(h('span', null, UI.loader('Loading'), ' Loading models…'));
      let r;
      try {
        r = existing && !k && !url.value.trim()
          ? await api.providerModels(existing.id)
          : await api.listModels({ id: existing?.id || '', baseUrl: base(), key: k, type: isClaude() ? 'anthropic' : '', free: !!preset.local });
      } catch (e) {
        r = { ok: false, error: e?.message || 'Could not load models' };
      }
      if (id !== loadId || !rows.isConnected) return;
      if (!r || !r.ok) { say(r?.error || 'Could not load models', 'bad'); return; }
      (r.models || []).forEach((m) => { if (!known.has(m.id)) known.set(m.id, m); });
      options = [...known.values()].sort((a, b) => a.label.localeCompare(b.label)).map((m) => ({ value: m.id, label: m.label + (m.task === 'stt' ? ' (speech)' : '') }));
      redraw();
      say(`${options.length} models`);
    }
    key.addEventListener('input', UI.debounce(load, 600));
    url.addEventListener('change', load);

    async function save(test) {
      const k = key.value.trim();
      const seenIds = new Set();
      const models = [];
      for (const id of picked()) {
        if (seenIds.has(id)) continue;
        seenIds.add(id);
        const m = known.get(id) || {};
        models.push({ tier: 'decent', ...m, id, label: m.label || id, task: m.task || guessTask(id) });
      }
      if (!models.length) return say('Choose at least one model.', 'bad');
      if (!k && !st.hasKey && !preset.keyOptional) return say('Paste your key first.', 'bad');
      if (preset.key === 'cloudflare' && !/^[0-9a-f]{32}$/i.test(acct.value.trim())) return say('The Account ID is 32 letters and numbers.', 'bad');
      if (isCustom() && (!name.value.trim() || !url.value.trim())) return say('Name and address are required.', 'bad');
      let p;
      if (existing) p = { ...existing };
      else if (isBuiltin()) {
        if (removed.includes(preset.key)) await api.saveProvider({ id: preset.key });
        p = { ...(App.cfg().providers.find((x) => x.id === preset.key) || { id: preset.key }) };
      } else {
        p = { id: preset.key + '-' + Math.random().toString(16).slice(2, 6), name: preset.name, keyLabel: preset.keyLabel || 'API key',
          keyOptional: !!preset.keyOptional, free: !!preset.local, docs: preset.docs, enabled: true, ...(preset.type ? { type: preset.type } : {}) };
      }
      if (isCustom()) p.name = name.value.trim();
      if (url.value.trim() || !p.baseUrl) p.baseUrl = base();
      if (preset.key === 'cloudflare') p.accountId = acct.value.trim();
      const prevModels = p.models || [];
      const chosenIds = new Set(models.map((m) => m.id));
      const removedModels = new Set(p.removedModels || []);
      for (const m of prevModels) if (!chosenIds.has(m.id)) removedModels.add(m.id);
      for (const id of chosenIds) removedModels.delete(id);
      p.models = models;
      p.removedModels = [...removedModels];
      await api.saveProvider(p);
      if (k) {
        const kr = await api.setKey(p.id, k);
        if (kr === false || (kr && kr.ok === false)) {
          const msg = kr?.error || 'Could not save API key.';
          say(msg, 'bad');
          UI.toast?.(msg, 'error');
          existing = p;
          refresh();
          return;
        }
      }
      // only the models picked here are listed on the page
      await api.set('shownModels', [...(App.cfg().shownModels || []).filter((r) => !r.startsWith(p.id + '::')), ...models.map((m) => `${p.id}::${m.id}`)]);
      if (!test) { close(); UI.toast(`${p.name} saved`, 'ok'); refresh(); return; }
      say(h('span', null, UI.loader('Testing'), ' Testing…'));
      const r = await api.testProvider(p.id);
      if (!r.ok) { existing = p; refresh(); return say(r.error || 'That key did not work.', 'bad'); }
      close();
      refresh();
      UI.toast(`${p.name} connected`, 'ok');
    }

    const remove = existing ? h('button', { class: 'lnk danger', type: 'button', on: { click: () => UI.confirm(`Remove ${existing.name}?`).then((ok) => { if (ok) { close(); api.removeProvider(existing.id).then(refresh); } }) } }, 'Remove') : '';
    const close = UI.modal(h('div', { class: 'svc-form' },
      h('div', { class: 'flex between' }, h('h3', { style: { margin: 0 } }, existing ? existing.name : 'New AI service'), UI.btn('', { icon: 'x', variant: 'icon ghost', title: 'Close', onClick: () => close() })),
      h('label', null, 'Provider'), provider,
      nameWrap,
      h('label', null, 'API URL (optional)'), url, urlHint,
      acctWrap, keyWrap, h('div', null, consoleLink),
      h('label', null, 'Model'), rows, h('div', null, more),
      h('div', { class: 'hint' }, 'Each model gets its own card and shares the URL and key above.'),
      h('div', { class: 'svc-foot' }, status, remove, UI.btn('Save', { onClick: () => save(false) }), UI.btn('Save and test', { variant: 'primary', onClick: () => save(true) }))));
    fill();
    if (existing) load();
    setTimeout(() => (keyWrap.hidden ? url : key).focus(), 80);
  }
  // brand marks, matched by id/type prefix
  const LOGOS = Object.fromEntries(['anthropic', 'cloudflare', 'openai', 'openrouter', 'groq', 'xai', 'mistral', 'together', 'fireworks', 'cerebras',
    'deepseek', 'gemini', 'opencode', 'ollama', 'lmstudio'].map((k) => [k, k + '.svg']).concat([['murmint', '../logo.svg']]));
  const TINTS = ['#ff6a1f', '#3f8fd6', '#8d6bd8', '#3fae86', '#e0a43a', '#d65a8a', '#c2553a', '#6b7280'];
  function logo(key, name, big) {
    const k = Object.keys(LOGOS).find((x) => (key || '').toLowerCase().startsWith(x));
    let hash = 0;
    for (const c of name || '?') hash = (hash * 31 + c.charCodeAt(0)) >>> 0;
    // how each mark sits in its tile: square marks fill it, black or white ones flip with the theme
    const fit = { groq: ' full', openai: ' ink', openrouter: ' ink', ollama: ' ink', opencode: ' ink', xai: ' paper' }[k] || '';
    return h('span', { class: `plogo${big ? ' big' : ''}${k ? ' img' + fit : ''}`, style: k ? null : { background: TINTS[hash % TINTS.length] } },
      k ? h('img', { src: `../assets/providers/${LOGOS[k]}`, alt: '' }) : (name || '?').trim()[0].toUpperCase());
  }
  const logoKey = (p) => (p.builtin ? p.type : p.id);
  function statusOf(p) {
    const st = App.S.providerStatus[p.id] || {};
    if (!p.enabled) return ['Off', ''];
    if (st.ready) return ['Ready', 'good'];
    return [st.hasKey ? 'Check settings' : 'Needs a key', 'warn'];
  }

  function backups(path, refs, task, exclude) {
    const wrap = h('div', { class: 'backups' }, h('span', { class: 'muted' }, 'If it fails, try'));
    refs.forEach((r, i) => wrap.append(h('span', { class: 'chip' }, h('b', null, String(i + 1)), App.refLabel(r),
      h('button', { type: 'button', title: 'Remove', on: { click: () => App.set(path, refs.filter((_, j) => j !== i), { render: true }) } }, icon('x')))));
    const opts = App.modelOptions(task).map((g) => ({ ...g, options: g.options.filter((o) => !refs.includes(o.value) && !exclude.includes(o.value)) })).filter((g) => g.options.length);
    if (opts.length) {
      const add = UI.select([{ value: '', label: refs.length ? 'Add another' : 'Add a backup' }, ...opts], '', (v) => v && App.set(path, [...refs, v], { render: true }), 'w-sm');
      add.classList.add('backup-add');
      wrap.append(add);
    }
    return wrap;
  }

  const LOCAL = 'murmint-local';
  const M1 = `${LOCAL}::whisper-tiny`;
  const EDGE_ID = 'murmint-edge', EDGE = `${EDGE_ID}::sensevoice-small`;
  const onThisPc = (p) => p && (p.id === LOCAL || p.id === EDGE_ID);

  // Murmint's own speech models (Murmint 1 and Murmint Edge), shown inside the Speech panel: download, progress and
  // delete. Progress repaints only that model's line.
  const LOCAL_MODELS = {
    m1: { key: undefined, name: 'Murmint 1', note: 'Free, private, works offline.' },
    edge: { key: 'edge', name: 'Murmint Edge', note: 'Faster and better with accents. Free, private, works offline.' },
  };
  const localSt = { m1: null, edge: null }, paintLocal = {};
  api.on('local:progress', (p) => {
    const id = p.model === 'edge' ? 'edge' : 'm1';
    if (!localSt[id]) return;
    localSt[id] = { ...localSt[id], ...p, installed: p.state === 'installed' || (localSt[id].installed && p.state !== 'none') };
    if (paintLocal[id]) paintLocal[id]();
  });
  function modelLine(id) {
    const { key, name, note } = LOCAL_MODELS[id];
    const line = h('div', { class: 'm1' });
    const draw = () => {
      const st = localSt[id];
      if (!st) { line.replaceChildren(h('span', { class: 'muted' }, `Checking ${name}…`)); return; }
      const mb = (n) => Math.max(1, Math.round(n / 1024 ** 2)) + ' MB';
      let right;
      if (st.state === 'downloading') {
        const pct = st.total ? Math.min(100, Math.round(((st.received || 0) / st.total) * 100)) : 0;
        right = h('div', { class: 'm1-dl' }, h('div', { class: 'onb-bar' }, h('i', { style: { width: pct + '%' } })), h('span', { class: 'muted tnum' }, pct + '%'),
          UI.btn('Cancel', { variant: 'ghost sm', onClick: () => api.cancelModels(key) }));
      } else if (!st.installed) {
        right = UI.btn(`Download ${mb(st.size)}`, { variant: 'sm', icon: 'download', onClick: async () => { localSt[id] = { ...localSt[id], ...(await api.downloadModels(key)) }; draw(); } });
      } else {
        right = h('div', { class: 'm1-dl' }, h('span', { class: 'm1-ok' }, window.icon('check'), 'On this computer'),
          UI.btn('', { icon: 'trash', variant: 'ghost icon', title: `Delete ${name}`, onClick: () => removeModel(id) }));
      }
      line.replaceChildren(h('img', { src: '../assets/logo.svg', alt: '' }),
        h('div', { class: 'grow' }, h('b', null, name), h('span', null, st.error || note)), right);
    };
    paintLocal[id] = draw;
    draw();
    return line;
  }
  async function removeModel(id) {
    const { key, name } = LOCAL_MODELS[id];
    if (!(await UI.confirm(`Delete ${name} from this computer? You can download it again any time.`, 'Delete'))) return;
    localSt[id] = { ...localSt[id], ...(await api.removeModel(key)), installed: false };
    App.S = await api.getState();
    App.render();
  }

  // One job (speech or formatting): what it uses now, and how that is chosen.
  // One job (speech or formatting): the model in use and its provider at the top, and one menu to choose how:
  // automatically, the cheapest, or a model of your own.
  const AUTO = '@auto', CHEAP = '@cheapest';
  function jobPanel({ task, cfgKey, extra }) {
    const job = App.cfg().models[cfgKey];
    const path = `models.${cfgKey}`;
    const value = job.routing === 'manual' ? job.primary : job.routing === 'cheapest' ? CHEAP : AUTO;
    const ref = task === 'stt' ? preview?.stt?.ref || (job.routing === 'manual' ? job.primary : '') :
      job.routing === 'manual' ? job.primary : preview?.chat?.[0]?.ref;
    const p = App.providerOfRef(ref);
    const ready = p && (onThisPc(p) || App.S.providerStatus[p.id]?.ready);

    // formatting offers the models you picked under AI services; speech offers every speech model of a ready service,
    // so streaming models can be picked without adding them first
    const shown = new Set(App.cfg().shownModels || []);
    const readyRef = (ref) => !!App.S.providerStatus[App.providerOfRef(ref)?.id]?.ready;
    const offered = (o) => shown.has(o.value) || o.value === job.primary || (task === 'stt' && readyRef(o.value));
    const groups = App.modelOptions(task).map((g) => ({ ...g, options: g.options.filter(offered) })).filter((g) => g.options.length);
    const options = [
      { value: AUTO, label: 'Automatic pick' },
      { value: CHEAP, label: 'Cheapest' },
      ...(task === 'stt' ? [{ group: 'On this computer', options: [{ value: M1, label: 'Murmint 1 (free)' },
        ...(App.cfg().providers.some((x) => x.id === EDGE_ID) ? [{ value: EDGE, label: 'Murmint Edge (free)' }] : [])] }] : []),
      ...groups.map((g) => ({ ...g, options: g.options.filter((o) => o.value !== M1 && o.value !== EDGE) })).filter((g) => g.options.length),
    ];
    const pick = (v) => {
      if (v === AUTO || v === CHEAP) App.set(`${path}.routing`, v === AUTO ? 'auto' : 'cheapest', { render: true });
      else App.set(path, { ...job, routing: 'manual', primary: v }, { render: true });
    };
    const how = { [AUTO]: 'Murmint picks the best model you have.', [CHEAP]: 'Murmint picks the cheapest model you have.' }[value] || 'You picked this model.';

    const panel = h('section', { class: 'mjob' },
      h('div', { class: 'mjob-now' },
        h('div', { class: 'grow' },
          h('div', { class: 'mjob-model' }, ref ? App.refLabel(ref) : 'Nothing set up yet'),
          h('div', { class: 'mjob-via' }, h('span', { class: `dot ${ready ? 'good' : 'warn'}` }), ref ? (onThisPc(p) ? 'On this computer' : p?.name || 'Removed provider') : 'Add an AI service below',
            task === 'stt' && preview?.stt?.stream ? ' · Live' : '')),
        h('div', { class: 'mjob-pickbox' }, UI.select(options, value, pick, 'w-full'), h('span', null, how))));
    if (job.routing === 'manual') panel.append(h('div', { class: 'mjob-pick' }, backups(`${path}.fallbacks`, job.fallbacks || [], task, [job.primary])));
    if (extra) panel.append(...[].concat(extra).filter(Boolean));
    return panel;
  }


  // The page is redrawn from the engine's saved state after every change made here, so it never shows old data
  async function refresh() {
    App.S = await api.getState();
    App.renderSidebar();
    App.render();
  }

  // One line per provider; open one to choose which of its models to use, one or many at once.
  const results = {}; // provider id -> { ms } or { error } or 'testing'
  const openRows = new Set();
  const speed = (r) => {
    if (r === 'testing') return h('span', { class: 'ms testing' }, '…');
    if (!r) return null;
    if (r.error) return h('span', { class: 'ms bad', title: r.error }, 'Failed');
    return h('span', { class: `ms ${r.ms < 900 ? 'fast' : r.ms < 2000 ? 'mid' : 'slow'}` }, r.ms < 1000 ? `${r.ms}ms` : `${(r.ms / 1000).toFixed(1)}s`);
  };

  // A menu of the provider's models with a tick box each; nothing is saved until Add
  function modelMenu(p) {
    const refs = new Set((App.cfg().shownModels || []).filter((r) => r.startsWith(p.id + '::')));
    const known = new Map(p.models.map((m) => [m.id, m]));
    const box = h('div', { class: 'mmenu' });
    const search = UI.input({ placeholder: 'Search models' });
    const list = h('div', { class: 'mmenu-list' });
    const count = h('span', { class: 'muted' });
    const note = h('span', { class: 'muted' });
    const draw = () => {
      const q = search.value.trim().toLowerCase();
      const all = [...known.values()].sort((a, b) => a.label.localeCompare(b.label));
      const hits = all.filter((m) => !q || m.label.toLowerCase().includes(q) || m.id.toLowerCase().includes(q)).slice(0, 300);
      list.replaceChildren(...hits.map((m) => {
        const ref = `${p.id}::${m.id}`;
        const cb = h('input', { type: 'checkbox' });
        cb.checked = refs.has(ref);
        cb.addEventListener('change', () => { cb.checked ? refs.add(ref) : refs.delete(ref); count.textContent = `${refs.size} selected`; });
        return h('label', { class: 'mmenu-item' }, cb, h('span', { class: 'grow' }, m.label), h('span', { class: 'muted' }, m.task === 'stt' ? 'speech' : 'text'));
      }));
      if (!hits.length) list.append(h('div', { class: 'muted', style: { padding: '10px' } }, q ? 'Nothing matches that.' : 'No models yet.'));
      count.textContent = `${refs.size} selected`;
    };
    search.addEventListener('input', draw);
    draw();
    note.replaceChildren(UI.loader('Loading'), ' Loading the full list…');
    api.providerModels(p.id).then((r) => {
      if (!box.isConnected) return;
      if (!r || !r.ok) { note.textContent = r?.error || 'Could not load models'; return; }
      (r.models || []).forEach((m) => { if (!known.has(m.id)) known.set(m.id, m); });
      note.textContent = `${known.size} models`;
      draw();
    }).catch(() => {
      if (!box.isConnected) return;
      note.textContent = 'Could not load models';
    });
    const add = UI.btn('Add selected', { variant: 'sm primary', onClick: async () => {
      add.disabled = true;
      const existingMap = new Map((p.models || []).map((m) => [m.id, m]));
      const picked = [...known.values()].filter((m) => refs.has(`${p.id}::${m.id}`));
      const models = picked.map((m) => existingMap.get(m.id) || { tier: 'decent', ...m });
      const checkedIds = new Set(models.map((m) => m.id));
      const removedModels = new Set(p.removedModels || []);
      for (const m of p.models || []) if (!checkedIds.has(m.id)) removedModels.add(m.id);
      for (const id of checkedIds) removedModels.delete(id);
      await api.saveProvider({ ...p, models, removedModels: [...removedModels] });
      await api.set('shownModels', [...(App.cfg().shownModels || []).filter((r) => !r.startsWith(p.id + '::')), ...models.map((m) => `${p.id}::${m.id}`)]);
      UI.toast(`${models.length} model${models.length === 1 ? '' : 's'} from ${p.name}`, 'ok');
      await refresh();
    } });
    box.append(search, list, h('div', { class: 'mmenu-foot' }, count, note, h('span', { class: 'grow' }), add));
    return box;
  }

  function servicesPanel() {
    const shownRefs = new Set(App.cfg().shownModels || []);
    const services = App.cfg().providers.filter((p) => !onThisPc(p) && (!p.builtin || App.S.providerStatus[p.id]?.hasKey || p.models.some((m) => shownRefs.has(`${p.id}::${m.id}`))));
    const ready = services.filter((p) => App.S.providerStatus[p.id]?.ready).length;
    const fmt = App.cfg().models.format;
    const inUse = fmt.routing === 'manual' ? fmt.primary : preview?.chat?.[0]?.ref;
    const list = h('div', { class: 'mlist2' });
    const paint = () => {
      list.replaceChildren(...services.flatMap((p, i) => {
        const [label, kind] = statusOf(p);
        const picked = p.models.filter((m) => shownRefs.has(`${p.id}::${m.id}`));
        const open = openRows.has(p.id);
        const using = picked.some((m) => `${p.id}::${m.id}` === inUse);
        const row = h('div', { class: `mline${open ? ' open' : ''}`, role: 'button', tabindex: 0, 'aria-expanded': String(open), style: { '--i': i } },
          logo(logoKey(p), p.name),
          h('div', { class: 'grow' }, h('b', null, p.name, using ? h('span', { class: 'in-use' }, 'In use') : ''),
            h('span', null, picked.length ? picked.map((m) => m.label).join(', ') : 'No models chosen yet')),
          speed(results[p.id]) || h('span', { class: `ms ${kind}` }, label),
          h('button', { class: 'btn icon', type: 'button', title: 'Key and address', 'aria-label': `Edit ${p.name}`, on: { click: (e) => { e.stopPropagation(); serviceModal(p); } } }, icon('gear')),
          h('span', { class: 'chev' }, icon('chevron')));
        const toggle = () => { open ? openRows.delete(p.id) : openRows.add(p.id); paint(); };
        row.addEventListener('click', toggle);
        row.addEventListener('keydown', (e) => { if (e.key === 'Enter') toggle(); });
        return open ? [row, modelMenu(p)] : [row];
      }));
    };
    // every service is tested at the same time, and the page is redrawn once, when they have all answered
    const testAll = UI.btn('Test all', { icon: 'refresh', variant: 'sm', onClick: async () => {
      testAll.disabled = true;
      services.forEach((p) => { results[p.id] = 'testing'; });
      paint();
      App._hold = true;
      try {
        await Promise.all(services.map(async (p) => {
          const t0 = performance.now();
          const r = await api.testProvider(p.id);
          results[p.id] = r.ok ? { ms: r.ms || Math.round(performance.now() - t0) } : { error: r.error || 'Failed' };
        }));
      } finally {
        App._hold = false;
      }
      await refresh();
    } });
    testAll.disabled = !services.length;
    paint();
    return h('section', { class: 'svc section' },
      h('div', { class: 'svc-head' },
        h('div', null, h('h2', null, 'AI services', UI.badge(ready ? `${ready} ready` : 'None ready', ready ? 'good' : '')),
          h('p', null, 'Open a provider to choose its models. Keys stay encrypted on this PC.')),
        h('div', { class: 'svc-acts' }, testAll, UI.btn('Add provider', { icon: 'plus', variant: 'sm primary', onClick: () => serviceModal() }))),
      services.length ? list : h('div', { class: 'svc-empty' }, 'No providers yet. Click Add provider; OpenRouter is a good starting point.'));
  }

  // What Automatic would pick and Murmint 1's state are fetched before the page is drawn, so a change shows the
  // loader once and then the finished page, instead of a page that fills in and flashes.
  let preview = null, shownFor = '', loading = false, justLoaded = false, tab = 'stt';
  const keyOf = () => {
    const cfg = App.cfg();
    return JSON.stringify([
      cfg.models,
      (cfg.providers || []).map((p) => [
        p.id,
        p.enabled,
        App.S.providerStatus[p.id]?.ready,
        (p.models || []).map((m) => m.id),
        p.removedModels || [],
      ]),
      cfg.dictation?.realtime,
      cfg.dictation?.language,
      cfg.privacy?.localOnly,
    ]);
  };
  async function load(key) {
    loading = true;
    try {
      const [r, m1, edge] = await Promise.all([api.routerPreview(), localSt.m1 || api.localModels(), localSt.edge || api.localModels('edge'), new Promise((ok) => setTimeout(ok, 260))]);
      preview = r; localSt.m1 = m1; localSt.edge = edge; shownFor = key; justLoaded = true;
    } catch { shownFor = key; }
    loading = false;
    if (App.page === 'models') App.render();
  }

  Pages.models = {
    render(root, App) {
      const key = keyOf();
      if (key !== shownFor) {
        const main = document.getElementById('main'); // centred in what is on screen, wherever the page was scrolled to
        root.append(h('div', { class: 'page-loader', 'aria-label': 'Loading', style: { top: main.scrollTop + 'px', bottom: 'auto', height: main.clientHeight + 'px' } }, ...Array.from({ length: 5 }, () => h('i'))));
        if (!loading) load(key);
        return;
      }
      if (justLoaded) { justLoaded = false; root.classList.add('m-in'); }
      const cfg = App.cfg();
      const m = cfg.models;
      root.append(UI.pageHead('Models', 'Murmint 1 turns your voice into text on this computer. Your providers format it.', 'Voice'));

      const longText = m.format.routing === 'manual' ? h('div', { class: 'longtext' },
        h('div', { class: 'flex between' }, h('div', null, h('b', null, 'Stronger model for long text'), h('span', null, 'Lists, emails and anything long go to a better model.')),
          UI.toggle(m.format.autoRoute, (v) => App.set('models.format.autoRoute', v, { render: true }))),
        m.format.autoRoute ? h('div', { class: 'flex wrap', style: { marginTop: '12px', gap: '10px' } },
          UI.select(App.modelOptions('chat', { allowEmpty: true }), m.format.complex, (v) => App.set('models.format.complex', v, { render: true }), 'w-md'),
          h('span', { class: 'muted' }, 'after'),
          UI.input({ type: 'number', value: m.format.complexWordThreshold, cls: 'w-sm', onChange: (v) => App.set('models.format.complexWordThreshold', Number(v) || 120) }),
          h('span', { class: 'muted' }, 'words')) : '') : null;

      root.append(h('div', { class: 'tabs', role: 'tablist' }, [['stt', 'Speech to text', 'mic'], ['chat', 'Formatting', 'textsize']].map(([id, label, ic]) =>
        h('button', { class: 'tab', role: 'tab', 'aria-selected': String(tab === id), on: { click: () => { tab = id; App.render(); } } }, icon(ic), label))));
      root.append(tab === 'stt'
        ? jobPanel({ task: 'stt', cfgKey: 'stt', extra: [modelLine('m1'), modelLine('edge')] })
        : jobPanel({ task: 'chat', cfgKey: 'format', extra: [longText] }));

      root.append(servicesPanel());

    },
  };
})();
