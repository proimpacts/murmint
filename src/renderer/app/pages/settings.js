(function () {
  const { h, icon } = UI;
  const api = window.murmint;

  const DEFAULT_KEYS = { dictate: 'Ctrl+Win', handsFree: 'Ctrl+Alt+D', cancel: 'Escape', undo: 'Ctrl+Alt+Z', formatSelection: 'Ctrl+Alt+F' };
  const CODE_NAMES = { Space: 'Space', Escape: 'Escape', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', ArrowLeft: 'Left', ArrowRight: 'Right', ArrowUp: 'Up', ArrowDown: 'Down', Insert: 'Insert', Delete: 'Delete', Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown', Semicolon: ';', Equal: '=', Comma: ',', Minus: '-', Period: '.', Slash: '/', Backquote: '`', BracketLeft: '[', Backslash: '\\', BracketRight: ']', Quote: "'" };
  const MODS = { Control: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Meta: 'Win' };

  function nameOf(e) {
    if (MODS[e.key]) return MODS[e.key];
    if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
    if (/^Digit\d$/.test(e.code)) return e.code.slice(5);
    if (/^F\d{1,2}$/.test(e.code)) return e.code;
    return CODE_NAMES[e.code] || null;
  }
  const ORDER = ['Ctrl', 'Alt', 'Shift', 'Win'];
  const sortCombo = (keys) => [...keys].sort((a, b) => { const ia = ORDER.indexOf(a), ib = ORDER.indexOf(b); return (ia < 0 ? 9 : ia) - (ib < 0 ? 9 : ib); });

  // Murmint account, signed in with Discord through murmint.app. Signing out goes back to the sign-in screen.
  function accountRow() {
    const holder = h('div');
    const draw = (user) => holder.replaceChildren(user
      ? UI.row('Account', `Signed in as ${user.name}.`, UI.btn('Sign out', { variant: 'ghost sm', onClick: async () => { await api.signOut(); location.reload(); } }))
      : UI.row('Account', 'Sign in with Discord to use Murmint.', UI.btn('Sign in', { variant: 'primary sm', onClick: signIn })));
    async function signIn() {
      const codeBox = h('div', { class: 'link-code' }, '…');
      const close = UI.modal(h('div', null, h('h3', null, 'Sign in with Discord'),
        h('p', { class: 'muted' }, 'Your browser opened murmint.app. Sign in and check that this code matches:'),
        codeBox,
        h('div', { class: 'flex mt3', style: { justifyContent: 'flex-end' } }, UI.btn('Cancel', { variant: 'ghost', onClick: () => { api.cancelSignIn(); close(); } }))));
      const off = api.on('account:code', (c) => { codeBox.textContent = c.userCode; });
      const r = await api.signIn();
      off(); close();
      if (r.ok) { UI.toast(`Signed in as ${r.user.name}`, 'ok'); draw(r.user); }
      else if (!r.cancelled) UI.toast(r.error || 'Could not sign in', 'error');
    }
    api.account().then(draw);
    return holder;
  }

  function hotkeyField(name) {
    const holder = h('div', { class: 'hk', tabindex: 0, role: 'button', 'aria-label': 'Change ' + name + ' shortcut' });
    const show = () => { holder.classList.remove('recording'); holder.replaceChildren(UI.kbds(App.cfg().hotkeys[name])); };
    let recording = false, timer = null, busy = false, combo = new Set();
    const stop = async () => { recording = false; clearInterval(timer); timer = null; await api.captureKeys(false); show(); };
    const finish = async () => {
      const keys = sortCombo(combo); const str = keys.join('+');
      const hasMod = keys.some((k) => ORDER.includes(k));
      const special = keys.length === 1 && (/^F\d+$/.test(keys[0]) || keys[0] === 'Escape');
      await stop();
      if (!hasMod && !special) return UI.toast('Add Ctrl, Alt, Shift or Win so it doesn’t clash with typing.', 'error');
      const clash = Object.entries(App.cfg().hotkeys).find(([k, v]) => k !== name && v === str);
      if (clash) return UI.toast('Already used by “' + clash[0] + '”.', 'error');
      App.cfg().hotkeys[name] = str;
      App.set('hotkeys.' + name, str); show(); App.renderSidebar();
    };
    const tick = async () => {
      if (busy || !recording) return; busy = true;
      try {
        const keys = await api.keysDown();
        if (!recording) return;
        if (keys.length) { keys.forEach((k) => combo.add(k)); holder.replaceChildren(UI.kbds(sortCombo(combo).join('+'))); }
        else if (combo.size) await finish();
      } finally { busy = false; }
    };
    holder.addEventListener('click', async () => {
      if (recording) return;
      recording = true; combo = new Set();
      holder.classList.add('recording'); holder.replaceChildren(h('span', { class: 'muted' }, 'Hold the keys, then let go…'));
      await api.captureKeys(true);
      timer = setInterval(tick, 40);
    });
    holder.addEventListener('blur', () => { if (recording) stop(); });
    show();
    return h('div', { class: 'flex' }, holder, UI.btn('', { icon: 'refresh', variant: 'ghost icon', title: 'Reset to default', onClick: () => { App.set('hotkeys.' + name, DEFAULT_KEYS[name], { render: true }); App.renderSidebar(); } }));
  }

  function micSection(cfg) {
    const sel = h('div', { class: 'ctl' }, h('span', { class: 'muted' }, 'Looking for microphones…'));
    const meter = h('div', { class: 'meter' }, h('i'));
    const note = h('span', { class: 'muted' });
    let off = null, timer = null;
    const stopTest = () => { clearTimeout(timer); if (off) off(); off = null; api.micTest(false, 'default'); meter.firstChild.style.width = '0'; testBtn.lastChild.textContent = 'Test microphone'; };
    const testBtn = UI.btn('Test microphone', { icon: 'mic', onClick: () => {
      if (off) { stopTest(); return; }
      let peak = 0;
      api.micTest(true, App.cfg().dictation.micDeviceId);
      off = api.on('mic:level', (v) => { peak = Math.max(peak, v); meter.firstChild.style.width = Math.min(100, v * 100) + '%'; });
      testBtn.lastChild.textContent = 'Stop test'; note.textContent = 'Say something…';
      timer = setTimeout(() => { stopTest(); note.textContent = peak > 0.15 ? 'Looks good, your microphone is working.' : 'Very quiet. Check the selected device and your Windows input volume.'; }, 8000);
    } });
    api.audioDevices().then((dl) => {
      sel.replaceChildren();
      if (!dl.ok) { sel.append(h('span', { class: 'muted' }, 'No microphone found')); return; }
      sel.append(UI.select([{ value: 'default', label: dl.defaultName ? `System default (${dl.defaultName})` : 'System default' }, ...dl.devices.map((d) => ({ value: d.id, label: d.name }))], cfg.dictation.micDeviceId, (v) => App.set('dictation.micDeviceId', v), 'w-lg'));
    });
    return UI.card(
      h('div', { class: 'row' }, h('div', { class: 'grow' }, h('div', { class: 't' }, 'Microphone'), h('div', { class: 'd' }, 'Murmint only listens while you dictate; the mic is released a few seconds after.')), sel),
      h('div', { class: 'row stack' }, h('div', { class: 'flex' }, testBtn, note), meter),
      UI.row('Silence threshold', 'Quieter recordings are ignored.', UI.seg([{ value: 0.005, label: 'Sensitive' }, { value: 0.010, label: 'Normal' }, { value: 0.025, label: 'Noisy room' }], cfg.dictation.noiseGate, (v) => App.set('dictation.noiseGate', v))),
      UI.row('AutoType', 'No text box selected? Murmint finds the app\'s message box.', UI.toggle(cfg.dictation.autoType !== false, (v) => App.set('dictation.autoType', v))),
      UI.row('Stop after silence','Ends click and hands-free dictation after this much quiet.', UI.seg([{ value: 0, label: 'Never' }, { value: 2000, label: '2 s' }, { value: 3000, label: '3 s' }, { value: 5000, label: '5 s' }], cfg.dictation.autoStopMs === undefined ? 3000 : cfg.dictation.autoStopMs, (v) => App.set('dictation.autoStopMs', v))),
      UI.row('Pause that ends a sentence', 'Hands-free: how long a pause before text goes in.', UI.seg([{ value: 700, label: 'Short' }, { value: 1100, label: 'Normal' }, { value: 1800, label: 'Long' }], cfg.dictation.silenceMs, (v) => App.set('dictation.silenceMs', v))));
  }

  let tab = 'general';
  const TABS = [['general', 'General', 'tab-general'], ['dictation', 'Dictation', 'tab-dictation'], ['shortcuts', 'Shortcuts', 'tab-shortcuts'], ['insertion', 'Typing', 'tab-typing'], ['privacy', 'Privacy', 'tab-privacy'], ['appearance', 'Appearance', 'tab-appearance']];

  const group = (title, desc, ...rows) => h('section', { class: 'section' }, h('h2', null, title), desc ? h('p', { class: 'desc' }, desc) : null, UI.card(...rows));

  const canLive = (cfg) => cfg.providers.some((p) => App.S.providerStatus[p.id] && App.S.providerStatus[p.id].ready && p.type !== 'cloudflare' && p.models.some((m) => m.task === 'stt' && m.realtime));

  const panels = {
    general(cfg) {
      return [
        group('Account', null, accountRow()),
        group('Startup', null,
          UI.row('Start with Windows', 'Opens in the tray, ready for your shortcut.', UI.toggle(cfg.general.launchAtLogin, (v) => App.set('general.launchAtLogin', v))),
          UI.row('Keep running when closed', 'Closing the window keeps Murmint in the tray.', UI.toggle(cfg.general.runInBackground, (v) => App.set('general.runInBackground', v)))),
        group('Updates', null,
          UI.row('Check for updates', 'Offers new versions. Nothing installs without your click.', UI.toggle(cfg.general.autoUpdate !== false, (v) => App.set('general.autoUpdate', v))),
          UI.row('Check now', 'Look for a new version now.', UI.btn('Check now', { variant: 'ghost', size: 'sm', onClick: async (e) => { const b = e.currentTarget; b.disabled = true; const u = await window.murmint.checkUpdate().catch(() => null); b.disabled = false; if (!u) UI.toast('Could not check for updates.', 'error'); else if (u.none) UI.toast(u.error || 'You have the latest version.', u.error ? 'error' : 'ok'); else UI.toast('Murmint ' + u.version + ' is available.', 'info'); } }))),
        group('About', null,
          UI.row('Version', null, h('span', { class: 'muted' }, App.S.version))),
      ];
    },
    dictation(cfg) {
      return [
        group('How you dictate', null,
          UI.row('Live transcription', canLive(cfg) ? 'Words appear while you speak.' : 'Words appear while you speak. Needs an OpenAI key.', UI.toggle(cfg.dictation.realtime !== false, (v) => App.set('dictation.realtime', v))),
          UI.row('Speech support', 'For stutters and speech differences. Waits longer and reads carefully.', UI.toggle(cfg.dictation.speechSupport === true, (v) => App.set('dictation.speechSupport', v), 'Speech support')),
          UI.row('Noise reduction', 'Removes fans, hum and hiss.', UI.toggle(cfg.dictation.noiseReduction !== false, (v) => App.set('dictation.noiseReduction', v))),
          UI.row('Hold to talk', 'Off: press once to start, again to finish.', UI.toggle(cfg.dictation.trigger === 'hold', (v) => App.set('dictation.trigger', v ? 'hold' : 'toggle'))),
          UI.row('Live preview', 'A running caption while you speak.', UI.toggle(cfg.dictation.livePreview, (v) => App.set('dictation.livePreview', v)))),
        group('Language', null,
          UI.row('Spoken language', 'Or let Murmint detect it.', UI.select(LANGS.map(([v, l]) => ({ value: v, label: l })), cfg.dictation.language, (v) => App.set('dictation.language', v), 'w-md'))),
        h('section', { class: 'section' }, h('h2', null, 'Microphone'), micSection(cfg)),
      ];
    },
    shortcuts(cfg) {
      return [group('Keyboard shortcuts', 'Click a shortcut, then press the keys you want. They work in every app.',
        UI.row('Dictate', cfg.dictation.trigger === 'hold' ? 'Hold while speaking.' : 'Press to start, press again to finish.', hotkeyField('dictate')),
        UI.row('Hands-free mode', 'Keeps listening. Press again to stop.', hotkeyField('handsFree')),
        UI.row('Cancel', 'Throws the recording away.', hotkeyField('cancel')),
        UI.row('Undo last insertion', 'Takes back what Murmint just typed.', hotkeyField('undo')),
        UI.row('Format selection', 'Cleans up the text you select.', hotkeyField('formatSelection')))];
    },
    insertion(cfg) {
      return [group('Inserting text', null,
        UI.row('Better Clipboard', 'Win+V opens your copies, emojis and GIFs.', UI.toggle(cfg.clipboard?.enabled !== false, (v) => { cfg.clipboard = { ...cfg.clipboard, enabled: v }; api.setBetterClipboard(v); })),
        UI.row('Restore my clipboard', 'Puts back what you had copied.', UI.toggle(cfg.insertion.restoreClipboard, (v) => App.set('insertion.restoreClipboard', v))),
        UI.row('Add a trailing space', 'Handy for several phrases in a row.', UI.toggle(cfg.insertion.trailingSpace, (v) => App.set('insertion.trailingSpace', v))))];
    },
    privacy(cfg) {
      return [
        group('What is sent', 'Audio is never saved. It goes to your speech provider, and the text to your formatting provider. With Message context on, the end of what is already in the box goes to the formatting provider too. Nowhere else.',
          UI.row('Tell the AI which app I am in', 'Matches the tone. Window titles stay private.', UI.toggle(cfg.privacy.sendAppContext, (v) => App.set('privacy.sendAppContext', v))),
          UI.row('Send my vocabulary to providers', 'Used as spelling hints.', UI.toggle(cfg.privacy.sendVocabulary, (v) => App.set('privacy.sendVocabulary', v)))),
        group('History', 'Stored only on this PC.',
          UI.row('Save dictation history', null, UI.toggle(cfg.privacy.saveHistory, (v) => App.set('privacy.saveHistory', v))),
          UI.row('Keep history for', null, UI.select([{ value: 7, label: '7 days' }, { value: 30, label: '30 days' }, { value: 90, label: '90 days' }, { value: 0, label: 'Until I delete it' }], cfg.privacy.retentionDays, (v) => App.set('privacy.retentionDays', Number(v)), 'w-md')),
          h('div', { class: 'flex mt2' }, UI.btn('Clear history', { icon: 'trash', variant: 'danger', onClick: () => { UI.confirm('Delete all saved dictations?').then((ok) => { if (ok) api.clearHistory().then(() => UI.toast('History cleared', 'ok')); }); } }),
            UI.btn('Reset usage stats', { variant: 'ghost', onClick: () => { UI.confirm('Reset usage statistics?').then((ok) => { if (ok) api.resetUsage().then(() => UI.toast('Reset', 'ok')); }); } }))),
      ];
    },
    appearance(cfg) {
      const sw = (id, color) => h('button', { class: 'swatch', style: { background: color }, 'aria-pressed': String(cfg.appearance.accent === id), title: id, 'aria-label': `${id} accent`, on: { click: () => { App.set('appearance.accentPicked', true); App.set('appearance.accent', id, { render: true }); } } });
      return [
        group('Look', null,
          UI.row('Theme', null, UI.seg([{ value: 'system', label: 'Auto' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }], cfg.appearance.theme, (v) => App.set('appearance.theme', v))),
          UI.row('Accent', 'Orange by default.', h('div', { class: 'flex' }, [['ember', '#ff6a1f'], ['mono', 'linear-gradient(135deg,#f4f4f5 50%,#0c0c0e 50%)'], ['mint', '#4fd1a5'], ['sky', '#5aa9ff'], ['rose', '#ff6f91']].map(([i, c]) => sw(i, c))))),
        group('Dictation pill', 'The small floating pill near the edge of your screen shows when Murmint is listening. Drag it to move it; it settles on the nearest edge: left, right, top or bottom.',
          UI.row('Show when idle', 'Off: the pill only shows while you dictate.', UI.toggle(cfg.appearance.pillIdle !== false, (v) => App.set('appearance.pillIdle', v))),
          UI.row('Side of the screen', null, UI.seg([{ value: 'left', label: 'Left' }, { value: 'right', label: 'Right' }, { value: 'top', label: 'Top' }, { value: 'bottom', label: 'Bottom' }], ['left', 'right', 'top', 'bottom'].includes(cfg.appearance.pillSide) ? cfg.appearance.pillSide : 'right', (v) => App.set('appearance.pillSide', v))),
          UI.row('Sounds', 'Soft cues when listening starts and stops.', UI.toggle(cfg.appearance.sounds, (v) => App.set('appearance.sounds', v))),
          UI.row('Reduce motion', 'Fewer animations.', UI.toggle(cfg.appearance.reduceMotion, (v) => App.set('appearance.reduceMotion', v)))),
      ];
    },
  };

  Pages.settings = {
    render(root, App) {
      const cfg = App.cfg();
      root.append(UI.pageHead('Settings', 'How Murmint listens, types and looks.'));
      root.append(h('div', { class: 'tabs', role: 'tablist' }, TABS.map(([id, label, ic]) => h('button', { class: 'tab', role: 'tab', 'aria-selected': String(tab === id), on: { click: () => { tab = id; App.render(); } } }, icon(tab === id ? `${ic}:on` : ic), label))));
      root.append(...panels[tab](cfg));
    },
  };
})();
