(function () {
  const { h } = UI;
  const api = window.murmint;
  const PRESETS = window.AppPresets || [];
  let editId = null;

  const STRENGTH = [
    { value: '', label: 'Default' },
    { value: '0', label: 'Verbatim' },
    { value: '1', label: 'Light' },
    { value: '2', label: 'Balanced' },
    { value: '3', label: 'Strong' },
  ];

  const ICONS = {
    discord: 'discord.svg', slack: 'slack.svg', teams: 'teams.svg', outlook: 'outlook.svg', gmail: 'gmail.png',
    'ai-chat': 'claude.svg', vscode: 'vscode.svg', cursor: 'cursor.svg', windsurf: 'windsurf.svg', jetbrains: 'jetbrains.svg', visualstudio: 'visualstudio.svg',
    terminal: 'terminal.svg', word: 'word.svg', notion: 'notion.svg', notepad: 'notepad.svg', browser: 'chrome.svg',
  };

  window.APP_ICONS = ICONS; // Home uses the same icons for its recent list

  const appIcon = (p) => h('span', { class: 'app-ico' },
    p.icon ? h('img', { src: p.icon, alt: '' })
      : ICONS[p.id] ? h('img', { src: `../assets/apps/${ICONS[p.id]}`, alt: '' })
        : window.icon('apps'));

  const overrideOf = (id) => App.cfg().appOverrides?.[id] || {};
  const styleName = (cfg, id) => cfg.styles.find((s) => s.id === id)?.name || 'Natural';

  // must match the engine's norm_proc
  const normProgram = (p) => String(p).trim().toLowerCase().split(/[\\/]/).pop().replace(/\.exe$/, '');
  const byName = (a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });

  // mirrors is_added in profiles.rs
  const isAdded = (p) => {
    const ov = overrideOf(p.id);
    return p.custom === true || ov.added === true || !!ov.style || typeof ov.intensity === 'number' || !!ov.instruction || !!ov.rules?.length;
  };

  const fallbackStyle = (cfg, p) => styleName(cfg, p.style || cfg.formatting.style);

  const setOverride = (id, key, value, render) =>
    App.set(`appOverrides.${id}.${key}`, value, render ? { render: true } : undefined);

  function summary(cfg, p) {
    const ov = overrideOf(p.id);
    if (ov.enabled === false) return 'Off. This app uses your default writing style.';
    const bits = [`${ov.style && cfg.styles.some((x) => x.id === ov.style) ? styleName(cfg, ov.style) : fallbackStyle(cfg, p)} style`];
    if (typeof ov.intensity === 'number') bits.push(STRENGTH[ov.intensity + 1].label.toLowerCase());
    if (ov.instruction) bits.push(ov.overrideRules ? 'only your notes' : 'your notes');
    const rules = (ov.rules || []).filter((r) => r.enabled !== false && r.text?.trim()).length;
    if (rules) bits.push(rules === 1 ? '1 rule' : `${rules} rules`);
    return bits.join(', ');
  }

  function appRow(cfg, p) {
    const ov = overrideOf(p.id);
    const off = ov.enabled === false;
    const styleOptions = [
      { value: '', label: `Default (${fallbackStyle(cfg, p)})` },
      ...cfg.styles.map((s) => ({ value: s.id, label: s.name })),
    ];
    const row = h('div', { class: 'row' },
      h('div', { class: 'grow app-id' }, appIcon(p),
        h('div', null, h('div', { class: 't' }, p.name, off && UI.badge('Off')), h('div', { class: 'd' }, summary(cfg, p)))),
      h('div', { class: 'ctl' },
        off ? null : UI.select(styleOptions, ov.style && cfg.styles.some((x) => x.id === ov.style) ? ov.style : '', (v) => setOverride(p.id, 'style', v, true)),
        UI.btn(editId === p.id ? 'Done' : 'Edit', { variant: 'ghost sm', onClick: () => { editId = editId === p.id ? null : p.id; App.render(); } })));
    if (editId !== p.id) return [row];

    const saveNotes = UI.debounce((v) => setOverride(p.id, 'instruction', v));
    const programs = (p.process || []).join(', ') || 'anything with a matching window title';
    const panel = h('div', { class: 'row stack' },
      UI.row('Use these settings', 'Turn off to make this app follow your default writing style.',
        UI.toggle(!off, (v) => setOverride(p.id, 'enabled', v, true), `${p.name} settings`)),
      UI.row('Strength', 'How much gets rewritten in this app. Default follows Writing.',
        UI.select(STRENGTH, typeof ov.intensity === 'number' ? String(ov.intensity) : '',
          (v) => setOverride(p.id, 'intensity', v === '' ? null : Number(v), true))),
      h('div', { class: 'd' }, 'Notes for this app. Murmint is told these every time it writes here. Leave empty to keep the built-in note.'),
      UI.textarea({ value: ov.instruction || '', placeholder: p.instruction || 'e.g. Keep it short. No sign-off.', rows: 3,
        onInput: (v) => saveNotes(v) }),
      h('div', { class: 'd' }, 'Rules for this app. They are followed here on top of your rules from Writing.'),
      window.ruleEditor(`app-${p.id}`, () => overrideOf(p.id).rules || [], (list, render) => setOverride(p.id, 'rules', list, render)),
      UI.row('Only use these notes and rules', 'Turn on to leave out your rules from Writing in this app, so only the notes and rules above apply.',
        UI.toggle(ov.overrideRules === true, (v) => setOverride(p.id, 'overrideRules', v), `${p.name} notes only`)),
      h('div', { class: 'd' }, `Matches: ${programs}`),
      h('div', { class: 'flex' },
        UI.btn('Reset to default', { variant: 'ghost sm', onClick: () => { App.set(`appOverrides.${p.id}`, p.custom ? {} : { added: true }, { render: true }); } }),
        UI.btn('Remove from list', { variant: 'ghost danger sm', onClick: async () => {
          if (!(await UI.confirm(`Remove ${p.name}? Murmint goes back to your default style there.`, 'Remove'))) return;
          editId = null;
          if (p.custom) App.set('appProfiles', App.cfg().appProfiles.filter((x) => x.id !== p.id));
          App.set(`appOverrides.${p.id}`, {}, { render: true });
        } })));
    return [row, panel];
  }

  function addAppModal() {
    const cfg = App.cfg();
    const profiles = cfg.appProfiles || [];
    const procsOf = (p) => (p.process || []).map(normProgram);
    const addedProcs = new Set(profiles.filter(isAdded).flatMap(procsOf));
    const builtinFor = (proc) => profiles.find((p) => !p.custom && procsOf(p).includes(proc));

    const entries = [];
    const popularHere = new Set();
    const popularEntry = (pr) => {
      if (popularHere.has(pr.id)) return;
      popularHere.add(pr.id);
      const builtin = pr.builtin && profiles.find((p) => p.id === pr.builtin);
      const done = builtin ? isAdded(builtin) : pr.process.some((q) => addedProcs.has(normProgram(q)));
      entries.push({ key: `preset:${pr.id}`, group: 'popular', name: pr.name, icon: `../assets/apps/${pr.icon}`, process: pr.process, kind: pr.kind, style: pr.style, builtin, done });
    };
    for (const pr of PRESETS) {
      const builtin = pr.builtin && profiles.find((p) => p.id === pr.builtin);
      if (builtin ? isAdded(builtin) : pr.process.some((q) => addedProcs.has(normProgram(q)))) popularEntry(pr);
    }
    const presetFor = (a) => PRESETS.find((pr) => pr.process.some((q) => normProgram(q) === normProgram(a.process)) || pr.name.toLowerCase() === a.name.toLowerCase());
    // matched by window title
    for (const p of profiles) {
      if (!p.custom && !(p.process || []).length) entries.push({ key: `web:${p.id}`, group: 'web', name: p.name, icon: ICONS[p.id] ? `../assets/apps/${ICONS[p.id]}` : null, builtin: p, done: isAdded(p) });
    }

    const seen = new Set();
    let chosen = null;
    let style = '';
    let filter = '';
    let loading = true;
    const tiles = new Map();
    const list = h('div', { class: 'app-pick' });
    const note = h('div', { class: 'd' }, 'Also looking for the apps on your PC…');

    const select = (key) => {
      chosen = key;
      tiles.forEach((t, k) => t.classList.toggle('on', k === key));
    };

    const paint = () => {
      if (!list.isConnected) return;
      tiles.clear();
      list.innerHTML = '';
      const shown = (group) => entries.filter((e) => e.group === group && (!filter || e.name.toLowerCase().includes(filter))).sort(byName);
      const tile = (e) => {
        const t = h('button', { type: 'button', class: `app-tile${e.key === chosen ? ' on' : ''}${e.done ? ' done' : ''}`, on: { click: () => { if (!e.done) select(e.key); } } },
          appIcon({ ...(e.builtin || {}), icon: e.icon }), h('span', { class: 'app-name' }, e.name), e.done ? h('span', { class: 'app-added' }, 'Added') : null);
        tiles.set(e.key, t);
        return t;
      };
      let any = false;
      for (const [group, title] of [['popular', 'Popular apps on this PC'], ['pc', 'All apps on this PC'], ['web', 'On the web']]) {
        const rows = shown(group);
        if (!rows.length) continue;
        any = true;
        list.append(h('div', { class: 'app-pick-head' }, title));
        rows.forEach((e) => list.append(tile(e)));
      }
      if (!any && !loading) list.append(h('div', { class: 'd', style: { padding: '8px 4px', gridColumn: '1 / -1' } }, 'Nothing matches that. Use "Not listed?" below to add it yourself.'));
    };

    const addFound = (found) => {
      for (const a of found) {
        if (seen.has(a.process) || seen.has(a.name.toLowerCase())) continue;
        seen.add(a.process);
        seen.add(a.name.toLowerCase());
        const pr = presetFor(a);
        if (pr) { popularEntry(pr); continue; }
        const builtin = builtinFor(a.process);
        entries.push({ key: `pc:${a.process}`, group: 'pc', name: a.name, process: [a.process], icon: a.icon, kind: 'auto', style: '', builtin, done: builtin ? isAdded(builtin) : addedProcs.has(a.process) });
      }
      paint();
    };

    let wrap;
    let close;

    const styleSelect = () => UI.select(
      [{ value: '', label: 'Usual for this app' }, ...cfg.styles.map((s) => ({ value: s.id, label: s.name }))],
      style, (v) => { style = v; });

    const finish = (id) => { editId = id; close(); App.render(); };

    const createFromList = () => {
      const e = entries.find((x) => x.key === chosen);
      if (!e || e.done) { UI.toast('Pick an app first', 'error'); return; }
      if (e.builtin) {
        App.set(`appOverrides.${e.builtin.id}`, { added: true, ...(style ? { style } : {}) });
        finish(e.builtin.id);
        return;
      }
      const profile = { id: 'app-' + UI.uid().slice(0, 8), name: e.name, process: e.process, title: [], kind: e.kind || 'auto', style: style || e.style || '', intensity: null, instruction: '', custom: true, ...(e.icon ? { icon: e.icon } : {}) };
      App.set('appProfiles', [...App.cfg().appProfiles, profile]);
      finish(profile.id);
    };

    // manual add
    function showManual() {
      style = '';
      let icon = null;
      const name = UI.input({ placeholder: 'Name, e.g. My editor' });
      const program = UI.input({ placeholder: 'Program, e.g. myeditor' });
      const browse = async () => {
        const found = await api.pickProgram();
        if (!found) return;
        program.value = found.process;
        if (!name.value.trim()) name.value = found.name;
        icon = found.icon;
      };
      const create = () => {
        const label = name.value.trim();
        const proc = normProgram(program.value);
        if (!label || !proc) { UI.toast('Add a name and choose the program', 'error'); return; }
        const taken = profiles.find((p) => isAdded(p) && procsOf(p).includes(proc));
        if (taken) { UI.toast(`${taken.name} already covers that program.`, 'error'); return; }
        const profile = { id: 'app-' + UI.uid().slice(0, 8), name: label, process: [proc], title: [], kind: 'auto', style, intensity: null, instruction: '', custom: true, ...(icon ? { icon } : {}) };
        App.set('appProfiles', [...App.cfg().appProfiles, profile]);
        finish(profile.id);
      };
      wrap.replaceChildren(
        h('h3', null, 'Add an app yourself'),
        h('div', { class: 'd mt2' }, 'Choose the program file, or type the name it has in Task Manager under Details, without .exe.'),
        h('div', { class: 'grid mt2', style: { gap: '10px' } }, name,
          h('div', { class: 'flex' }, program, UI.btn('Choose file…', { variant: 'ghost sm', onClick: browse }))),
        h('div', { class: 'mt3' }, h('div', { class: 'd' }, 'Writing style'), styleSelect()),
        h('div', { class: 'flex mt3', style: { justifyContent: 'flex-end' } },
          UI.btn('Back', { variant: 'ghost', onClick: showList }),
          UI.btn('Add', { variant: 'primary', onClick: create })));
      setTimeout(() => name.focus(), 40);
    }

    function showList() {
      style = '';
      const search = UI.input({ placeholder: 'Search apps', value: filter, onInput: (v) => { filter = v.trim().toLowerCase(); paint(); } });
      note.style.display = loading ? '' : 'none';
      wrap.replaceChildren(
        h('h3', null, 'Add an app'),
        h('div', { class: 'd mt2' }, 'Pick an app, then choose how Murmint writes in it.'),
        h('div', { class: 'mt2' }, search),
        note, list,
        h('div', { class: 'mt3' }, h('div', { class: 'd' }, 'Writing style'), styleSelect()),
        h('div', { class: 'flex mt3', style: { justifyContent: 'space-between' } },
          UI.btn('Not listed? Add it yourself', { variant: 'ghost sm', onClick: showManual }),
          h('div', { class: 'flex' },
            UI.btn('Cancel', { variant: 'ghost', onClick: () => close() }),
            UI.btn('Add', { variant: 'primary', onClick: createFromList }))));
      paint();
      setTimeout(() => search.focus(), 40);
    }

    wrap = h('div', null);
    close = UI.modal(wrap);
    showList();

    api.installedApps()
      .then((found) => { addFound(found); return api.installedStoreApps(); })
      .then((found) => { loading = false; note.style.display = 'none'; addFound(found); })
      .catch(() => { loading = false; note.style.display = 'none'; paint(); });
  }

  Pages.apps = {
    render(root, App) {
      const cfg = App.cfg();
      const added = (cfg.appProfiles || []).filter(isAdded);
      root.append(UI.pageHead('Apps', 'A writing style for each app.'));
      root.append(UI.section('Your apps',
        `Add the apps you want to treat differently. Everything else uses your default style, ${styleName(cfg, cfg.formatting.style)}, which you set under Writing.`,
        added.length
          ? UI.card(...added.flatMap((p) => appRow(cfg, p)))
          : UI.card(h('div', { class: 'muted', style: { padding: '28px 22px', textAlign: 'center' } }, 'No apps yet. Press Add, pick one of your apps, and give it its own writing style.')),
        h('div', { class: 'mt2' }, UI.btn('Add', { onClick: addAppModal }))));
    },
  };
})();
