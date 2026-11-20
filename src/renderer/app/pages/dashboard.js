(function () {
  const { h, icon } = UI;
  const api = window.murmint;

  window.LANGS = [
    ['auto', 'Auto-detect'], ['en', 'English'], ['es', 'Spanish'], ['fr', 'French'], ['de', 'German'], ['it', 'Italian'], ['pt', 'Portuguese'],
    ['nl', 'Dutch'], ['sv', 'Swedish'], ['da', 'Danish'], ['no', 'Norwegian'], ['fi', 'Finnish'], ['pl', 'Polish'], ['cs', 'Czech'], ['tr', 'Turkish'],
    ['ru', 'Russian'], ['uk', 'Ukrainian'], ['el', 'Greek'], ['he', 'Hebrew'], ['ar', 'Arabic'], ['hi', 'Hindi'], ['id', 'Indonesian'], ['vi', 'Vietnamese'],
    ['th', 'Thai'], ['ja', 'Japanese'], ['ko', 'Korean'], ['zh', 'Chinese'],
  ];

  const nf = new Intl.NumberFormat();
  const CHECK = '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 6.3 5 8.6 9.5 3.6"/></svg>';
  const ALERT = '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 3v3.6M6 8.9v.1"/></svg>';
  const TILES = ['#ff6a1f', '#e0a43a', '#c2553a', '#8d6bd8', '#3f8fd6', '#3fae86', '#d65a8a', '#6b7280'];

  function greeting() {
    const hr = new Date().getHours();
    if (hr < 5) return 'Up late';
    if (hr < 12) return 'Good morning';
    return hr < 18 ? 'Good afternoon' : 'Good evening';
  }

  const cell = (k, v, unit, note) =>
    h('div', { class: 'cell' }, h('div', { class: 'k' }, k), h('div', { class: 'v' }, v, unit && h('small', null, unit)), note && h('div', { class: 'n' }, note));

  function recentRow(e) {
    const sec = Math.round(e.sec || 0);
    const time = new Date(e.ts).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }).replace(/\s?[AP]M$/i, '');
    const row = h('div', { class: 'rec', title: 'Click to copy', role: 'button', tabindex: 0 },
      h('time', null, time),
      appTile(e),
      h('span', { class: 'tx' }, e.edited || e.text),
      h('span', { class: 'dur' }, `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, '0')}`));
    const copy = async () => {
      await api.copyText(e.edited || e.text);
      row.classList.add('copied');
      UI.toast('Copied', 'ok');
      setTimeout(() => row.classList.remove('copied'), 1200);
    };
    row.addEventListener('click', copy);
    row.addEventListener('keydown', (ev) => { if (ev.key === 'Enter') copy(); });
    return row;
  }

  // real exe icon > built-in icon > letter tile
  const NAME_HINTS = [[/discord/i, 'discord'], [/slack/i, 'slack'], [/teams/i, 'teams'], [/outlook|mail/i, 'outlook'], [/gmail/i, 'gmail'],
    [/visual studio code|vs ?code/i, 'vscode'], [/cursor/i, 'cursor'], [/windsurf/i, 'windsurf'], [/visual studio/i, 'visualstudio'],
    [/jetbrains|intellij|pycharm|webstorm/i, 'jetbrains'], [/terminal|powershell|cmd/i, 'terminal'], [/word/i, 'word'],
    [/notion|obsidian/i, 'notion'], [/notepad/i, 'notepad'], [/chatgpt|claude|ai chat/i, 'ai-chat'], [/chrome|edge|firefox|browser/i, 'browser']];
  function tint(name) {
    let hash = 0;
    for (const c of name) hash = (hash * 31 + c.charCodeAt(0)) >>> 0;
    return TILES[hash % TILES.length];
  }

  function appTile(e) {
    const name = e.app || 'App';
    const colour = tint(name);
    const tile = h('span', { class: 'tile', style: { background: colour } }, name.trim()[0]?.toUpperCase() || '?');
    const icons = window.APP_ICONS || {};
    const known = (e.profile && icons[e.profile]) || icons[(NAME_HINTS.find(([re]) => re.test(name)) || [])[1]];
    const show = (src) => { tile.textContent = ''; tile.classList.add('img'); tile.style.background = ''; tile.append(h('img', { src, alt: '' })); };
    if (e.exe) api.appIcon(e.exe).then((src) => { if (src) show(src); else if (known) show(`../assets/apps/${known}`); }).catch(() => { });
    else if (known) show(`../assets/apps/${known}`);
    return tile;
  }

  window.AppTile = appTile; // History shows the same icons

  function check(label, ok, right) {
    const mark = h('span', { class: 'mark' });
    mark.innerHTML = ok ? CHECK : ALERT;
    return h('div', { class: `chk ${ok ? 'ok' : 'bad'}` }, mark, label, right);
  }

  function statStrip(strip) {
    api.usage().then((u) => {
      if (!strip.isConnected) return;
      const week = (u.series || []).slice(-7);
      const today = week.at(-1)?.words || 0;
      const weekWords = week.reduce((a, d) => a + d.words, 0);
      const saved = u.savedMin >= 90 ? [(u.savedMin / 60).toFixed(1), 'h'] : [Math.round(u.savedMin), 'min'];
      const max = Math.max(1, ...week.map((d) => d.words));
      const spark = h('div', { class: 'spark' }, ...week.map((d, i) => h('i', {
        class: i === week.length - 1 ? 'today' : '',
        style: { height: `${8 + (d.words / max) * 26}px`, transformOrigin: '50% 100%' },
        title: `${d.day}: ${nf.format(d.words)} words`,
      })));
      const weekCell = cell('This week', nf.format(weekWords));
      weekCell.append(spark);
      strip.append(
        cell('Words today', nf.format(today), '', `${nf.format(u.month.words)} this month`),
        cell('Time saved', saved[0], saved[1], 'compared with typing'),
        cell('Speaking pace', u.wpmSpoken ? Math.round(u.wpmSpoken) : '-', 'wpm', 'typing is about 40'),
        weekCell);
      if (!window.Motion) return;
      const vals = strip.querySelectorAll('.v');
      Motion.countUp(vals[0], today, (x) => nf.format(x));
      Motion.countUp(vals[3], weekWords, (x) => nf.format(x));
      Motion.enter([...strip.children]);
      if (document.documentElement.dataset.motion !== 'reduced') {
        spark.querySelectorAll('i').forEach((b, i) => b.animate([{ transform: 'scaleY(0)' }, { transform: 'none' }],
          { duration: 520, delay: 320 + i * 50, easing: 'cubic-bezier(.2, .9, .25, 1.06)', fill: 'backwards' }));
      }
    });
  }

  Pages.dashboard = {
    render(root, App) {
      const cfg = App.cfg();
      const ready = App.anyReady();
      const date = new Date().toLocaleDateString([], { weekday: 'long', month: 'long', day: 'numeric' });

      root.append(h('div', { class: 'home-head' },
        h('div', null, h('span', { class: 'eyebrow' }, date), h('h1', null, greeting() + '.')),
        h('div', { class: 'home-hint' }, 'Hold', UI.kbds(cfg.hotkeys.dictate), 'anywhere to dictate')));
      const wave = h('div', { class: 'wave', 'aria-hidden': 'true' });
      root.append(wave);
      if (window.Motion) Motion.wave(wave);
      else wave.append(...Array.from({ length: 64 }, () => h('i')));

      if (!ready) {
        root.append(h('div', { class: 'banner', style: { marginTop: '28px' } }, icon('key'),
          h('div', { class: 'grow' }, h('div', { class: 't' }, 'Connect an AI provider to start'),
            h('div', { class: 'd' }, 'Murmint uses your own API key for transcription and formatting. Cloudflare Workers AI is the cheapest way to begin; OpenAI works too.')),
          UI.btn('Set up models', { variant: 'primary', onClick: () => App.nav('models') })));
      }

      const strip = h('div', { class: 'strip' });
      root.append(strip);
      statStrip(strip);

      const hist = App.S.history;
      const recent = h('div', { class: 'recent' });
      if (hist.length) hist.slice(0, 6).forEach((e) => recent.append(recentRow(e)));
      else recent.append(UI.empty('mic', 'Your dictations will show up here.'));
      const left = h('div', { class: 'section' },
        h('div', { class: 'sec-head' }, h('h2', null, 'Recent'), hist.length ? h('a', { class: 'link', on: { click: () => App.nav('history') } }, 'Open History') : null),
        recent);

      const style = cfg.styles.find((x) => x.id === cfg.formatting.style) || cfg.styles[0] || { name: 'Natural', description: '' };
      const styleCard = h('div', { class: 'card style-card' },
        h('span', { class: 'eyebrow' }, 'Active style'),
        h('h3', null, style.name),
        h('p', null, style.description || 'Your speech, cleaned up and formatted.'),
        UI.select(cfg.styles.map((x) => ({ value: x.id, label: x.name })), cfg.formatting.style, (v) => App.set('formatting.style', v, { render: true }), 'w-full'));

      const vocab = (cfg.personal.vocabulary || []).length;
      const rules = (cfg.formatting.rules || []).filter((r) => r.enabled !== false).length;
      const micRow = check('Microphone', true, h('span', { class: 'r' }, 'Checking…'));
      const checks = h('div', { class: 'checks' }, h('span', { class: 'eyebrow' }, 'Setup'),
        micRow,
        check('AI provider', ready, ready ? h('span', { class: 'r' }, 'Connected') : h('button', { class: 'lnk', on: { click: () => App.nav('models') } }, 'Set up')),
        check('Vocabulary', true, h('span', { class: 'r' }, `${vocab} word${vocab === 1 ? '' : 's'} · ${rules} rule${rules === 1 ? '' : 's'}`)));
      api.audioDevices().then((d) => {
        if (micRow.isConnected) micRow.replaceWith(check('Microphone', d.ok, h('span', { class: 'r' }, d.ok ? 'Ready' : 'Not found')));
      }).catch(() => { });

      root.append(h('div', { class: 'home-grid' }, left, h('div', null, styleCard, checks)));

      root.append(UI.section('Quick settings', null, UI.card(
        UI.row('Spoken language', null,
          UI.select(LANGS.map(([v, l]) => ({ value: v, label: l })), cfg.dictation.language, (v) => App.set('dictation.language', v), 'w-md')))));
    },
  };
})();
