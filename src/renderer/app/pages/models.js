(function () {
  const nf = (n) => new Intl.NumberFormat().format(n);
  const { h, icon } = UI;
  const api = window.murmint;

  // key modal: save + test in one step
  function keyModal(p) {
    const st = App.S.providerStatus[p.id] || {};
    const isCf = p.type === 'cloudflare';
    const acctOk = (v) => /^[0-9a-f]{32}$/i.test((v || '').trim());
    const key = UI.input({ type: 'password', placeholder: p.keyLabel || 'API key', mono: true, cls: 'w-full' });
    const acct = isCf ? UI.input({ value: p.accountId || '', placeholder: '32-character Account ID', mono: true, cls: 'w-full' }) : null;
    const msg = h('div', { class: 'key-msg' });
    const say = (text, kind) => { msg.className = `key-msg ${kind || ''}`; msg.replaceChildren(text); };
    if (isCf) {
      key.addEventListener('input', () => {
        if (/^[0-9a-f]{37}$/i.test(key.value.trim())) say('That looks like the Global API Key. Create an API Token instead (My Profile > API Tokens > Workers AI template).', 'warn');
        else say('');
      });
    }
    let close;
    const save = UI.btn(st.hasKey ? 'Save and test' : 'Add and test', { variant: 'primary', onClick: async () => {
      const value = key.value.trim();
      if (!value && !st.hasKey) return say('Paste your key first.', 'warn');
      if (isCf && !acctOk(acct.value) && !/[0-9a-f]{32}/i.test(acct.value)) return say('The Account ID is 32 letters and numbers.', 'warn');
      save.disabled = true;
      msg.className = 'key-msg';
      msg.replaceChildren(UI.loader('Testing'), ' Checking the key with ' + p.name + '…');
      if (isCf) { p.accountId = acct.value.trim(); await api.saveProvider(p); }
      if (value) await api.setKey(p.id, value);
      const r = await api.testProvider(p.id);
      save.disabled = false;
      if (!r.ok) return say(r.error || 'That key did not work.', 'bad');
      close();
      UI.toast(`${p.name} connected`, 'ok');
    } });
    key.addEventListener('keydown', (e) => { if (e.key === 'Enter') save.click(); });
    close = UI.modal(h('div', { class: 'key-modal' },
      h('div', { class: 'flex', style: { gap: '14px', marginBottom: '14px' } }, logo(logoKey(p), p.name, true),
        h('div', null, h('h3', { style: { margin: 0 } }, st.hasKey ? 'Change key' : 'Add your key'), h('div', { class: 'muted', style: { fontSize: '13px' } }, p.name))),
      isCf ? h('ol', { class: 'key-steps' },
        h('li', null, h('b', null, 'Account ID: '), 'in the Cloudflare dashboard open Workers AI, then Use REST API.'),
        h('li', null, h('b', null, 'API token: '), 'open ', h('a', { class: 'link', href: 'https://dash.cloudflare.com/profile/api-tokens', target: '_blank' }, 'My Profile > API Tokens'), ', Create Token, and use the Workers AI template.')) : '',
      acct || '',
      key,
      msg,
      h('div', { class: 'flex between mt3' },
        p.docs ? h('a', { class: 'link', href: p.docs, target: '_blank', style: { fontSize: '13px' } }, isCf ? 'Create a token' : 'Get an API key') : h('span'),
        h('div', { class: 'flex' }, UI.btn('Cancel', { variant: 'ghost', onClick: () => close() }), save)),
      h('p', { class: 'muted', style: { fontSize: '12px', margin: '14px 0 0' } }, 'Stored encrypted on this PC and only ever sent to ' + p.name + '.'),
      h('button', { class: 'lnk danger', type: 'button', on: { click: () => { UI.confirm(`Remove ${p.name}?`).then((ok) => { if (ok) { close(); api.removeProvider(p.id); } }); } } }, 'Remove this provider')));
    setTimeout(() => (acct && !acct.value ? acct : key).focus(), 60);
  }

  // brand marks, matched by id/type prefix
  const LOGOS = Object.fromEntries(['anthropic', 'cloudflare', 'openai', 'openrouter', 'groq', 'xai', 'mistral', 'together', 'fireworks', 'cerebras',
    'deepseek', 'gemini', 'opencode', 'ollama', 'lmstudio'].map((k) => [k, k + '.svg']).concat([['murmint', '../logo.svg']]));
  const TINTS = ['#ff6a1f', '#3f8fd6', '#8d6bd8', '#3fae86', '#e0a43a', '#d65a8a', '#c2553a', '#6b7280'];
  function logo(key, name, big) {
    const k = Object.keys(LOGOS).find((x) => (key || '').toLowerCase().startsWith(x));
    let hash = 0;
    for (const c of name || '?') hash = (hash * 31 + c.charCodeAt(0)) >>> 0;
    return h('span', { class: `plogo${big ? ' big' : ''}${k ? ' img' : ''}`, style: k ? null : { background: TINTS[hash % TINTS.length] } },
      k ? h('img', { src: `../assets/providers/${LOGOS[k]}`, alt: '' }) : (name || '?').trim()[0].toUpperCase());
  }
  const logoKey = (p) => (p.builtin ? p.type : p.id);
  function statusOf(p) {
    const st = App.S.providerStatus[p.id] || {};
    if (!p.enabled) return ['Off', ''];
    if (st.ready) return ['Ready', 'good'];
    return [st.hasKey ? 'Check settings' : 'Needs a key', 'warn'];
  }

  // add provider
  function addProviderModal() {
    let close, preset = null;
    const name = UI.input({ placeholder: 'Name' });
    const url = UI.input({ placeholder: 'https://api.example.com/v1', mono: true });
    const key = UI.input({ type: 'password', placeholder: 'API key', mono: true });
    const note = h('div', { class: 'muted', style: { fontSize: '12.5px', lineHeight: '1.5' } });
    const link = h('a', { class: 'link', target: '_blank', style: { fontSize: '12.5px' } });
    const advanced = h('details', { class: 'fold add-adv' },
      h('summary', { class: 'flex muted', style: { fontSize: '12.5px' } }, h('span', { class: 'chev' }, icon('chevron')), 'Name and address'),
      h('div', { class: 'grid', style: { gap: '8px', marginTop: '10px' } }, name, url));
    const picked = h('div', { class: 'add-picked' });
    const form = h('div', { class: 'add-form', hidden: true }, picked, key, note, link, advanced);
    const grid = h('div', { class: 'add-grid' }, h('span', { class: 'muted' }, 'Loading…'));
    const addBtn = UI.btn('Add provider', { variant: 'primary', onClick: async () => {
      if (!preset) return UI.toast('Pick a provider first', 'error');
      if (!name.value.trim() || !url.value.trim()) return UI.toast('Name and address are required', 'error');
      const id = (preset.key !== 'custom' ? preset.key : 'custom') + '-' + Math.random().toString(16).slice(2, 6);
      await api.saveProvider({
        id, name: name.value.trim(), baseUrl: url.value.trim(), keyLabel: preset.keyLabel || 'API key',
        keyOptional: preset.key !== 'custom' ? !!preset.keyOptional : !key.value, free: !!preset.local, docs: preset.docs, models: [],
      });
      if (key.value) await api.setKey(id, key.value);
      close();
      if (key.value) api.testProvider(id).then((r) => UI.toast(r.ok ? `${name.value.trim()} connected` : `Key check failed: ${r.error}`, r.ok ? 'ok' : 'error'));
      importModels(id);
    } });
    addBtn.disabled = true;
    api.providerPresets().then((list) => {
      const pick = (x, tile) => {
        preset = x;
        grid.querySelectorAll('.add-tile').forEach((t) => t.classList.toggle('on', t === tile));
        name.value = x.name; url.value = x.baseUrl || '';
        key.placeholder = x.keyOptional ? 'No key needed' : (x.keyLabel || 'API key');
        key.hidden = !!x.keyOptional && !!x.local;
        note.textContent = x.note || '';
        link.hidden = !x.docs; link.href = x.docs || '#';
        link.textContent = x.local ? `Download ${x.name}` : `Get a ${x.name} key`;
        advanced.open = x.key === 'custom';
        picked.replaceChildren(logo(x.key, x.name), h('b', null, x.key === 'custom' ? 'Any OpenAI-compatible server' : x.name), x.local ? UI.badge('Runs on this PC', 'good') : '');
        form.hidden = false; addBtn.disabled = false;
        if (!key.hidden) setTimeout(() => key.focus(), 50);
      };
      const removed = (App.cfg().removedProviders || []).map((id) => ({ id, name: { anthropic: 'Anthropic (Claude)', openai: 'OpenAI API', cloudflare: 'Cloudflare Workers AI' }[id] || id }));
      const restoreTiles = removed.map((r) => {
        const tile = h('button', { class: 'add-tile', type: 'button', title: 'Bring it back' }, logo(r.id, r.name), h('span', null, r.name));
        tile.addEventListener('click', async () => { await api.saveProvider({ id: r.id }); close(); UI.toast(`${r.name} is back`, 'ok'); });
        return tile;
      });
      grid.replaceChildren(...restoreTiles, ...list.map((x) => {
        const tile = h('button', { class: 'add-tile', type: 'button' }, logo(x.key, x.name), h('span', null, x.key === 'custom' ? 'Other' : x.name.replace(/\s*\(on this PC\)/i, '')), x.local ? h('small', null, 'local') : '');
        tile.addEventListener('click', () => pick(x, tile));
        return tile;
      }));
      if (window.Motion) Motion.enter([...grid.children].slice(0, 16));
    });
    close = UI.modal(h('div', { class: 'add-modal' },
      h('h3', null, 'Add a provider'),
      h('p', { class: 'muted', style: { margin: '0 0 16px' } }, 'Pick one, paste your key, then choose the models you want. Local ones run on this PC and cost nothing.'),
      grid, form,
      h('div', { class: 'flex mt3', style: { justifyContent: 'flex-end' } }, UI.btn('Cancel', { variant: 'ghost', onClick: () => close() }), addBtn)));
  }

  async function importModels(id) {
    const p = App.cfg().providers.find((x) => x.id === id);
    if (!p) return;
    const r = await api.providerModels(id);
    if (!r.ok) return UI.toast(r.error, 'error');
    const have = new Set(p.models.map((m) => m.id));
    const fresh = r.models.filter((m) => !have.has(m.id));
    if (!fresh.length) return UI.toast('No new models to add', 'info');
    const picked = new Set();
    let close;
    const filter = UI.input({ placeholder: 'Search ' + fresh.length + ' models' });
    const list = h('div', { style: { maxHeight: '340px', overflowY: 'auto', margin: '10px 0', display: 'grid', gap: '2px' } });
    const count = h('span', { class: 'muted' });
    const draw = () => {
      const q = filter.value.trim().toLowerCase();
      list.replaceChildren(...fresh.filter((m) => !q || m.id.toLowerCase().includes(q) || m.label.toLowerCase().includes(q)).slice(0, 250).map((m) => {
        const cb = h('input', { type: 'checkbox' }); cb.checked = picked.has(m.id);
        cb.addEventListener('change', () => { cb.checked ? picked.add(m.id) : picked.delete(m.id); count.textContent = picked.size + ' selected'; });
        const price = m.task === 'stt' ? (m.price ? 'speech' : 'speech, set a price') : m.price ? ('$' + m.price.inPerM + ' / $' + m.price.outPerM + ' per M') : 'text';
        return h('label', { class: 'flex', style: { gap: '10px', padding: '6px 8px', borderRadius: '8px', cursor: 'pointer' } }, cb, h('span', { class: 'grow' }, m.label), h('span', { class: 'muted', style: { fontSize: '12px' } }, price));
      }));
    };
    filter.addEventListener('input', draw); draw(); count.textContent = '0 selected';
    close = UI.modal(h('div', null,
      h('h3', null, 'Models from ' + p.name),
      h('p', { class: 'muted', style: { margin: '0' } }, 'Tick the ones you want. Prices come from the provider when it lists them; you can edit them afterwards.'),
      filter, list,
      h('div', { class: 'flex', style: { justifyContent: 'space-between' } }, count, h('div', { class: 'flex' },
        UI.btn('Cancel', { variant: 'ghost', onClick: () => close() }),
        UI.btn('Add selected', { variant: 'primary', onClick: async () => {
          const add = fresh.filter((m) => picked.has(m.id));
          if (!add.length) return UI.toast('Tick at least one model', 'error');
          await api.saveProvider({ ...p, models: [...p.models, ...add] });
          close(); App.render();
        } })))));
  }


  // model picker
  const TIER = { worst: 'Worst', bad: 'Bad', decent: 'Decent', good: 'Good', best: 'Best', cheap: 'Bad', balanced: 'Decent', capable: 'Best' };
  const priceOf = (m) => {
    const p = m.price || {};
    if (m.task === 'stt') return p.perMinute ? `$${p.perMinute}/min` : '';
    return p.inPerM || p.outPerM ? `$${p.inPerM ?? 0} / $${p.outPerM ?? 0} per 1M` : '';
  };

  function modelList(task, selected, onPick, { exclude = [] } = {}) {
    const box = h('div', { class: 'mlist' });
    const isReady = (p) => (App.S.providerStatus[p.id]?.ready ? 0 : 1); // ready providers first
    for (const p of [...App.cfg().providers].sort((a, b) => isReady(a) - isReady(b))) {
      const models = p.models.filter((m) => m.task === task && !exclude.includes(`${p.id}::${m.id}`));
      if (!models.length) continue;
      const ready = !!App.S.providerStatus[p.id]?.ready;
      box.append(h('div', { class: 'mgroup' }, logo(logoKey(p), p.name), h('span', null, p.name),
        ready ? '' : h('button', { class: 'lnk', type: 'button', on: { click: () => keyModal(p) } }, 'Add key')));
      for (const m of models) {
        const ref = `${p.id}::${m.id}`;
        const on = ref === selected;
        const row = h('button', { class: `mrow${on ? ' on' : ''}${ready ? '' : ' off'}`, type: 'button', 'aria-pressed': String(on) },
          h('span', { class: 'radio' }),
          h('span', { class: 'mname' }, m.label, m.realtime ? UI.badge('Live', 'accent') : ''),
          h('span', { class: `tier t-${m.tier || 'decent'}` }, TIER[m.tier || 'decent']),
          h('span', { class: 'mprice' }, priceOf(m)));
        row.addEventListener('click', () => { if (!ready) { keyModal(p); return; } onPick(ref); });
        box.append(row);
      }
    }
    if (!box.children.length) box.append(h('div', { class: 'muted', style: { padding: '14px 4px' } }, 'No models yet. Add a provider below.'));
    return box;
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

  // Murmint 1's download, shown inside the Speech panel. Progress repaints only that line.
  let localSt = null, paintLocal = null;
  api.on('local:progress', (p) => {
    if (!localSt) return;
    localSt = { ...localSt, ...p, installed: p.state === 'installed' || localSt.installed };
    if (paintLocal) paintLocal();
  });
  function murmint1Line() {
    const line = h('div', { class: 'm1' });
    const draw = () => {
      const st = localSt;
      if (!st) { line.replaceChildren(h('span', { class: 'muted' }, 'Checking Murmint 1…')); return; }
      const mb = (n) => Math.max(1, Math.round(n / 1024 ** 2)) + ' MB';
      let right;
      if (st.state === 'downloading') {
        const pct = st.total ? Math.min(100, Math.round(((st.received || 0) / st.total) * 100)) : 0;
        right = h('div', { class: 'm1-dl' }, h('div', { class: 'onb-bar' }, h('i', { style: { width: pct + '%' } })), h('span', { class: 'muted tnum' }, pct + '%'),
          UI.btn('Cancel', { variant: 'ghost sm', onClick: () => api.cancelModels() }));
      } else if (!st.installed) {
        right = UI.btn(`Download ${mb(st.size)}`, { variant: 'sm', icon: 'download', onClick: async () => { localSt = { ...localSt, ...(await api.downloadModels()) }; draw(); } });
      } else right = h('span', { class: 'm1-ok' }, window.icon('check'), 'On this computer');
      line.replaceChildren(h('img', { src: '../assets/logo.svg', alt: '' }),
        h('div', { class: 'grow' }, h('b', null, 'Murmint 1'), h('span', null, st.error || 'Free, private, works offline.')), right);
    };
    paintLocal = draw;
    draw();
    return line;
  }

  // One job (speech or formatting): what it uses now, and how that is chosen.
  function jobPanel({ task, title, ic, cfgKey, modes, extra }) {
    const job = App.cfg().models[cfgKey];
    const path = `models.${cfgKey}`;
    const local = job.routing === 'manual' && job.primary === M1;
    const mode = task === 'stt' && local ? 'murmint' : job.routing === 'manual' ? 'pick' : 'auto';
    const name = h('div', { class: 'mjob-model' }, '…'), via = h('div', { class: 'mjob-via' }), dot = h('span', { class: 'dot' });
    const setNow = (ref) => {
      const p = App.providerOfRef(ref);
      dot.className = `dot ${p && App.S.providerStatus[p.id]?.ready ? 'good' : 'warn'}`;
      name.textContent = ref ? App.refLabel(ref) : 'Nothing set up yet';
      via.textContent = ref ? (p?.id === LOCAL ? 'On this computer · free' : p?.name || 'Removed provider') : 'Add a provider below';
    };
    if (mode === 'auto') setNow(task === 'stt' ? preview?.stt?.ref : preview?.chat[0]?.ref);
    else setNow(job.primary);

    const choose = (m) => {
      if (m === mode) return;
      if (m === 'murmint') App.set(path, { ...job, primary: M1, routing: 'manual' }, { render: true });
      else if (m === 'auto') App.set(`${path}.routing`, 'auto', { render: true });
      else {
        const first = App.modelOptions(task).flatMap((g) => g.options).find((o) => o.value !== M1);
        App.set(path, { ...job, routing: 'manual', primary: job.primary && job.primary !== M1 ? job.primary : first ? first.value : '' }, { render: true });
      }
    };
    const seg = UI.seg(modes.map(([id, label]) => ({ value: id, label })), mode, choose);
    seg.classList.add('mjob-seg');
    const desc = h('p', { class: 'mjob-desc' }, (modes.find(([id]) => id === mode) || modes[0])[2]);

    const panel = h('section', { class: 'mjob' },
      h('div', { class: 'mjob-head' }, h('span', { class: 'job-ic' }, icon(ic)), h('h2', null, title)),
      h('div', { class: 'mjob-now' }, dot, h('div', { class: 'grow' }, name, via)),
      seg, desc);
    if (mode === 'pick') {
      const opts = App.modelOptions(task).map((g) => ({ ...g, options: g.options.filter((o) => o.value !== M1 || task === 'stt') })).filter((g) => g.options.length);
      panel.append(h('div', { class: 'mjob-pick' },
        opts.length ? UI.select(opts, job.primary, (v) => App.set(`${path}.primary`, v, { render: true }), 'w-full') : h('span', { class: 'muted' }, 'Add a provider below to pick a model.'),
        backups(`${path}.fallbacks`, job.fallbacks || [], task, [job.primary])));
    }
    if (extra) panel.append(...[].concat(extra).filter(Boolean));
    return panel;
  }

  function providersLine() {
    const ready = App.cfg().providers.filter((p) => p.id !== LOCAL && App.S.providerStatus[p.id]?.ready && p.models.some((m) => m.task === 'chat'));
    return h('div', { class: 'm1' }, h('span', { class: 'm1-ic' }, icon('key')),
      h('div', { class: 'grow' }, h('b', null, ready.length ? `${ready.length} provider${ready.length === 1 ? '' : 's'} ready` : 'No provider yet'),
        h('span', null, ready.length ? ready.map((p) => p.name).join(', ') : 'Add a key so Murmint can format your words.')),
      UI.btn('Add a provider', { variant: 'sm', icon: 'plus', onClick: addProviderModal }));
  }

  function providerTile(p) {
    const [label, kind] = statusOf(p);
    const speech = p.models.filter((m) => m.task === 'stt').length;
    const text = p.models.filter((m) => m.task === 'chat').length;
    const toggle = UI.toggle(p.enabled, (v) => { p.enabled = v; api.saveProvider(p); }, `${p.name} on`);
    toggle.addEventListener('click', (e) => e.stopPropagation());
    const tile = h('div', { class: 'ptile', role: 'button', tabindex: 0, title: 'Add or change the key' },
      h('div', { class: 'ptile-top' }, logo(logoKey(p), p.name), toggle),
      h('div', { class: 'ptile-name' }, p.name),
      h('div', { class: 'ptile-sub' }, [speech && `${speech} speech`, text && `${text} text`].filter(Boolean).join(' · ') || 'No models yet'),
      UI.badge(label, kind));
    tile.addEventListener('click', () => keyModal(p));
    tile.addEventListener('keydown', (e) => { if (e.key === 'Enter') keyModal(p); });
    return tile;
  }

  // What Automatic would pick and Murmint 1's state are fetched before the page is drawn, so a change shows the
  // loader once and then the finished page, instead of a page that fills in and flashes.
  let preview = null, shownFor = '', loading = false, justLoaded = false;
  const keyOf = () => JSON.stringify([App.cfg().models, App.cfg().providers.map((p) => [p.id, p.enabled, App.S.providerStatus[p.id]?.ready])]);
  async function load(key) {
    loading = true;
    try {
      const [r, st] = await Promise.all([api.routerPreview(), localSt ? localSt : api.localModels(), new Promise((ok) => setTimeout(ok, 260))]);
      preview = r; localSt = st; shownFor = key; justLoaded = true;
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

      root.append(h('div', { class: 'mjobs' },
        jobPanel({ task: 'stt', title: 'Speech to text', ic: 'mic', cfgKey: 'stt', extra: murmint1Line(), modes: [
          ['murmint', 'Murmint 1', 'Free, on this computer. The default.'],
          ['auto', 'Automatic', 'The best speech model among your providers.'],
          ['pick', 'Pick a model', 'Choose one yourself, with backups.']] }),
        jobPanel({ task: 'chat', title: 'Formatting', ic: 'textsize', cfgKey: 'format', extra: [longText, providersLine()], modes: [
          ['auto', 'Automatic', 'The best and cheapest model you have added.'],
          ['pick', 'Pick a model', 'Choose one yourself, with backups.']] })));

      const own = cfg.providers.filter((p) => p.id !== LOCAL);
      const grid = h('div', { class: 'pgrid' }, ...own.map(providerTile),
        h('button', { class: 'ptile add', type: 'button', on: { click: addProviderModal } },
          h('div', { class: 'ptile-top' }, h('span', { class: 'plogo plus' }, icon('plus'))),
          h('div', { class: 'ptile-name' }, 'Add a provider'), h('div', { class: 'ptile-sub' }, 'OpenRouter, Groq, Gemini, Ollama, LM Studio and more')));
      const section = UI.section('Your providers', 'Formatting uses these. Keys stay on this PC.');
      section.append(grid, h('p', { class: 'muted fine' }, 'OpenAI models need a key from platform.openai.com with its own billing. A ChatGPT Plus or Pro subscription does not include one.'));
      root.append(section);
      if (window.Motion) requestAnimationFrame(() => Motion.enter([...grid.children]));

      const usageBox = h('div', { class: 'card' }, h('div', { class: 'muted' }, 'Loading…'));
      root.append(UI.section('Usage this month', 'An estimate. Your provider’s bill is the real number.', usageBox));
      api.usage().then((u) => {
        if (!root.isConnected) return;
        const rows = Object.entries(u.byModel).sort((x, y) => y[1].cost - x[1].cost);
        usageBox.innerHTML = '';
        if (!rows.length) { usageBox.append(UI.empty('coin', 'No usage yet this month.')); return; }

        const compact = (n) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1).replace(/\.0$/, '')}k` : String(n));
        const minutes = (sec) => `${(sec / 60).toFixed(1).replace(/\.0$/, '')} min`;
        const audioSec = rows.reduce((n, [, v]) => n + (v.audioSec || 0), 0);
        const biggest = Math.max(...rows.map(([, v]) => v.cost), 1e-9);

        const row = ([ref, v]) => {
          const isText = v.tokensIn || v.tokensOut;
          const detail = [`${nf(v.requests)} ${v.requests === 1 ? 'request' : 'requests'}`,
            isText ? `${compact(v.tokensIn)} in, ${compact(v.tokensOut)} out` : v.audioSec ? minutes(v.audioSec) : null].filter(Boolean).join(' · ');
          return h('div', { class: 'usage-row' },
            h('div', { class: 'usage-name' }, h('b', null, App.refLabel(ref)), h('span', null, App.providerOfRef(ref)?.name || '')),
            h('div', { class: 'usage-bar', title: `${Math.round((v.cost / (u.month.cost || 1)) * 100)}% of this month's cost` }, h('i', { style: { width: `${Math.max(3, (v.cost / biggest) * 100)}%` } })),
            h('div', { class: 'usage-cost' }, h('b', null, UI.money(v.cost)), h('span', null, detail)));
        };
        const group = (title, list) => list.length ? [h('div', { class: 'usage-group' }, title), ...list.map(row)] : [];
        const text = rows.filter(([, v]) => v.tokensIn || v.tokensOut);
        const speech = rows.filter(([, v]) => !(v.tokensIn || v.tokensOut));

        usageBox.append(h('div', { class: 'usage' },
          h('div', { class: 'usage-sum' },
            h('div', null, h('b', null, UI.money(u.month.cost)), h('span', null, 'Estimated cost')),
            h('div', null, h('b', null, nf(u.month.requests)), h('span', null, 'Requests')),
            audioSec ? h('div', null, h('b', null, minutes(audioSec)), h('span', null, 'Audio transcribed')) : null),
          ...group('Speech to text', speech),
          ...group('Formatting', text),
          h('div', { class: 'usage-foot' },
            h('span', null, 'Resets on the 1st of each month.'),
            UI.btn('Reset usage counters', { variant: 'ghost sm', onClick: () => { UI.confirm('Reset all usage statistics?').then((ok) => { if (ok) api.resetUsage().then(() => App.render()); }); } }))));
      });
    },
  };
})();
