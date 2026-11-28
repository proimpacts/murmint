// First run: sign in with Discord (required), where they heard about Murmint, Murmint 1 downloading, then the shortcuts.
// The answers are saved as they are given, so closing the window picks up at the same step next time.
(function () {
  const { h } = UI;
  const api = window.murmint;

  const HEARD = [['discord', 'Discord'], ['youtube', 'YouTube'], ['tiktok', 'TikTok'], ['reddit', 'Reddit'], ['x', 'X (Twitter)'],
    ['github', 'GitHub'], ['search', 'A search engine'], ['friend', 'A friend'], ['other', 'Somewhere else']];
  // the shortcuts, asked one at a time
  const KEYS = [
    ['dictate', 'dictating', 'Hold them while you speak. Let go and your words appear.'],
    ['handsFree', 'hands-free dictation', 'Press once and Murmint keeps listening until you press again.'],
    ['cancel', 'cancelling a recording', 'Throws away what you were saying.'],
    ['undo', 'undoing the last dictation', 'Takes back the text Murmint just typed.'],
    ['formatSelection', 'formatting selected text', 'Select text you typed yourself and Murmint cleans it up.'],
  ];
  const ORDER = ['Ctrl', 'Alt', 'Shift', 'Win'];
  const sortCombo = (keys) => [...keys].sort((a, b) => { const ia = ORDER.indexOf(a), ib = ORDER.indexOf(b); return (ia < 0 ? 9 : ia) - (ib < 0 ? 9 : ib); });

  const size = (n) => Math.max(1, Math.round(n / 1024 ** 2)) + ' MB';

  function run(info) {
    return new Promise((finish) => {
      const root = document.getElementById('onboard');
      const app = document.getElementById('app');
      app.hidden = true; root.hidden = false;

      let user = info.user, local = info.local, edge = info.edge || { installed: false, size: 0 };
      // which speech models to download; Murmint 1 is ticked to start with
      const want = { m1: true, edge: false };
      let started = local.installed || edge.installed || local.state === 'downloading' || edge.state === 'downloading';
      const saved = App.cfg().onboarding || {};
      let heard = saved.heardFrom || '';
      let signing = null, keyIndex = 0;

      const steps = ['signin', 'heard', 'speech', 'keys'];
      let step = !user ? 'signin' : !heard ? 'heard' : !(local.installed || edge.installed) ? 'speech' : 'keys';

      const offs = [
        api.on('state:changed', (st) => { App.S = st; }),
        api.on('local:progress', (p) => {
          if (p.model === 'edge') edge = { ...edge, ...p, installed: p.state === 'installed' || (edge.installed && p.state !== 'none') };
          else local = { ...local, ...p, installed: p.state === 'installed' || (local.installed && p.state !== 'none') };
          if (step === 'speech') draw();
        }),
      ];
      const go = (to) => { recorder.stop(); step = to; draw(); };
      const next = () => go(steps[steps.indexOf(step) + 1]);
      const back = () => {
        if (step === 'keys' && keyIndex > 0) { keyIndex--; recorder.stop(); draw(); return; }
        go(steps[steps.indexOf(step) - 1]);
      };

      async function signIn() {
        signing = { code: null, url: null };
        draw();
        const off = api.on('account:code', (c) => { if (signing) { signing = { code: c.userCode, url: c.verifyUrl }; draw(); } });
        const r = await api.signIn();
        off();
        signing = null;
        if (r.ok) { user = r.user; next(); return; }
        draw();
        if (!r.cancelled) UI.toast(r.error || 'Could not sign in', 'error');
      }

      // Listens for the keys held down, and saves the combination once they are let go.
      const recorder = {
        on: false, timer: null, combo: new Set(), error: '',
        async start() {
          if (this.on) return;
          this.on = true; this.combo = new Set(); this.error = '';
          paintKeys();
          await api.captureKeys(true);
          let busy = false;
          this.timer = setInterval(async () => {
            if (busy || !this.on) return;
            busy = true;
            try {
              const keys = await api.keysDown();
              if (!this.on) return;
              if (keys.length) { keys.forEach((k) => this.combo.add(k)); paintKeys(); } else if (this.combo.size) this.done();
            } finally { busy = false; }
          }, 40);
        },
        stop() {
          if (!this.on) return;
          this.on = false; clearInterval(this.timer); api.captureKeys(false);
          paintKeys();
        },
        done() {
          const name = KEYS[keyIndex][0];
          const keys = sortCombo(this.combo), str = keys.join('+');
          this.combo = new Set();
          const special = keys.length === 1 && (/^F\d+$/.test(keys[0]) || keys[0] === 'Escape');
          if (!keys.some((k) => ORDER.includes(k)) && !special) { this.error = 'Add Ctrl, Alt, Shift or Win so it doesn’t clash with typing.'; paintKeys(); return; }
          const clash = KEYS.find(([k]) => k !== name && App.cfg().hotkeys[k] === str);
          if (clash) { this.error = `That’s already the shortcut for ${clash[1]}.`; paintKeys(); return; }
          this.error = '';
          App.set('hotkeys.' + name, str);
          this.stop();
        },
      };
      let keysBox = null;
      function paintKeys() {
        if (!keysBox) return;
        const name = KEYS[keyIndex][0];
        const showing = recorder.combo.size ? sortCombo(recorder.combo).join('+') : App.cfg().hotkeys[name];
        // the box is clicked to start listening, so keys pressed by accident never change a shortcut
        const box = h('button', { type: 'button', class: 'onb-rec' + (recorder.on ? ' on' : ''), 'aria-pressed': String(recorder.on),
          on: { click: () => (recorder.on ? recorder.stop() : recorder.start()) } },
        recorder.on && !recorder.combo.size ? h('span', { class: 'onb-rec-wait' }, 'Press your keys…') : UI.kbds(showing, true),
        h('span', { class: 'onb-rec-hint' }, recorder.on ? 'Listening. Let go to save, or click to stop' : 'Click to change'));
        keysBox.replaceChildren(box,
          h('p', { class: recorder.error ? 'onb-err' : 'onb-note' }, recorder.error || 'Click the box, hold the keys you want, then let go. Or keep these and continue.'));
      }

      const head = (title, sub) => h('div', { class: 'onb-head' }, h('h1', null, title), sub && h('p', null, sub));

      const VIEWS = {
        signin() {
          if (user) return [head(`Signed in as ${user.name}`, 'Continue to set up Murmint.')];
          const body = signing
            ? h('div', { class: 'onb-signin' },
              h('p', { class: 'onb-note' }, 'Your browser opened murmint.app. Sign in with Discord there and check that this code matches:'),
              h('div', { class: 'link-code' }, signing.code || '…'),
              h('div', { class: 'onb-wait' }, UI.loader('Waiting'), 'Waiting for you to confirm'),
              h('div', { class: 'flex' },
                signing.url && UI.btn('Open the page again', { variant: 'ghost sm', onClick: () => window.open(signing.url) }),
                UI.btn('Cancel', { variant: 'ghost sm', onClick: () => api.cancelSignIn() })))
            : h('div', { class: 'onb-signin' },
              h('button', { class: 'btn discord', on: { click: signIn } }, h('img', { src: '../assets/apps/discord.svg', alt: '' }), 'Sign in with Discord'),
              h('p', { class: 'onb-note' }, 'Murmint only gets your public Discord profile: your name and picture. What you say and type is never sent to murmint.app.'));
          return [head('Welcome to Murmint', 'Sign in with your Discord account to get started.'), body];
        },

        heard() {
          let other;
          const pick = (id) => { heard = id === 'other' ? (other && other.value.trim() ? 'other:' + other.value.trim() : 'other') : id; App.set('onboarding.heardFrom', heard); draw(); };
          const isOther = heard === 'other' || heard.startsWith('other:');
          const chips = HEARD.map(([id, label]) => h('button', {
            class: 'onb-chip', 'aria-pressed': String(id === 'other' ? isOther : heard === id), on: { click: () => pick(id) },
          }, label));
          const parts = [head('Where did you hear about Murmint?', 'This helps us know where to share it next.'), h('div', { class: 'onb-choices' }, chips)];
          if (isOther) {
            other = UI.input({ value: heard.slice(6), placeholder: 'Where? (optional)', cls: 'onb-other', onChange: (v) => { heard = v.trim() ? 'other:' + v.trim().slice(0, 80) : 'other'; App.set('onboarding.heardFrom', heard); } });
            parts.push(other);
          }
          return parts;
        },

        speech() {
          const MODELS = [
            ['m1', 'Murmint 1', 'Works well for most people. Small download.', () => local, (v) => { local = v; }, undefined],
            ['edge', 'Murmint Edge', 'Faster and better with accents. English, Chinese, Japanese, Korean and Cantonese.', () => edge, (v) => { edge = v; }, 'edge'],
          ];
          if (!started) {
            const rows = MODELS.map(([id, name, desc, st]) => {
              const box = h('input', { type: 'checkbox', checked: want[id], on: { change: (e) => { want[id] = e.target.checked; draw(); } } });
              return h('label', { class: 'onb-model' + (want[id] ? ' on' : '') }, box,
                h('div', { class: 'grow' }, h('b', null, name), h('span', null, desc)), h('span', { class: 'muted tnum' }, size(st().size || 0)));
            });
            return [head('Choose your speech models', 'They run on this computer, so turning your voice into text is free and private. You can download or delete them later under Models.'),
              h('div', { class: 'onb-models' }, ...rows)];
          }
          const lines = MODELS.filter(([id, , , st]) => want[id] || st().installed || st().state === 'downloading').map(([id, name, , st, set, key]) => {
            const m = st();
            if (m.installed) return h('div', { class: 'onb-wait good' }, window.icon('check'), `${name} is ready`);
            if (m.state === 'error') {
              return h('div', { class: 'onb-progress' }, h('div', { class: 'onb-err' }, `${name}: ${m.error}`),
                h('div', { class: 'flex', style: { justifyContent: 'center' } }, UI.btn('Try again', { variant: 'primary', onClick: async () => { set({ ...st(), ...(await api.downloadModels(key)) }); draw(); } })));
            }
            const pct = m.total ? Math.min(100, Math.round(((m.received || 0) / m.total) * 100)) : 0;
            return h('div', { class: 'onb-progress' }, h('b', { class: 'onb-model-name' }, name),
              h('div', { class: 'onb-bar' }, h('i', { style: { width: pct + '%' } })),
              h('div', { class: 'onb-dl-meta' }, h('span', null, m.checking ? 'Checking the download…' : `${size(m.received || 0)} of ${size(m.size || m.total)}`), h('span', null, pct + '%')));
          });
          return [head('Getting your speech models', 'This only happens once. You can keep going while they finish.'), ...lines];
        },

        keys() {
          const [name, label, what] = KEYS[keyIndex];
          keysBox = h('div', { class: 'onb-try' });
          paintKeys();
          return [h('div', { class: 'onb-head' }, h('span', { class: 'onb-count' }, `Shortcut ${keyIndex + 1} of ${KEYS.length}`),
            h('h1', null, `Press the keys you want for ${label}`), h('p', null, what)), keysBox,
          h('div', { class: 'flex', style: { justifyContent: 'center' } }, UI.btn('Use the default', { variant: 'ghost sm', onClick: () => {
            const def = { dictate: 'Ctrl+Win', handsFree: 'Ctrl+Alt+D', cancel: 'Escape', undo: 'Ctrl+Alt+Z', formatSelection: 'Ctrl+Alt+F' }[name];
            recorder.error = ''; App.set('hotkeys.' + name, def); paintKeys();
          } }))];
        },
      };

      function canContinue() {
        if (step === 'signin') return !!user;
        if (step === 'heard') return !!heard;
        if (step === 'speech') return started ? (!want.m1 || local.installed) && (!want.edge || edge.installed) : want.m1 || want.edge;
        return true;
      }

      async function onContinue(btn) {
        if (step === 'keys' && keyIndex < KEYS.length - 1) { recorder.stop(); keyIndex++; draw(); return; }
        if (step === 'speech' && !started) {
          // start the downloads that were ticked; Continue waits for them, Skip doesn't
          started = true;
          if (want.m1 && !local.installed) local = { ...local, ...(await api.downloadModels()) };
          if (want.edge && !edge.installed) edge = { ...edge, ...(await api.downloadModels('edge')) };
          draw();
          return;
        }
        if (step !== 'keys') return next();
        // the last shortcut ends setup and opens the app
        recorder.stop();
        btn.disabled = true;
        await api.finishOnboarding();
        offs.forEach((off) => off());
        root.hidden = true; root.replaceChildren(); app.hidden = false;
        finish({ page: 'models' });
      }

      function draw() {
        const i = steps.indexOf(step);
        const last = step === 'keys' && keyIndex === KEYS.length - 1;
        const cont = UI.btn(last ? 'Start using Murmint' : step === 'speech' && !started ? 'Download' : 'Continue', { variant: 'primary', onClick: () => onContinue(cont) });
        cont.classList.add('next');
        cont.disabled = !canContinue();
        const canBack = i > 0 && !(i === 1 && user) && !signing && step !== 'speech';
        // no download needed: speech can use an AI provider added under Models instead
        const skip = step === 'speech' && !(started && canContinue()) ? UI.btn('Skip', { variant: 'ghost', onClick: next }) : null;
        const foot = h('footer', { class: 'onb-foot' },
          canBack ? UI.btn('Back', { variant: 'ghost', onClick: back }) : skip || h('span'),
          h('div', { class: 'onb-dots', 'aria-label': `Step ${i + 1} of ${steps.length}` }, steps.map((_, n) => h('i', { class: n <= i ? 'on' : '' }))),
          cont);
        foot.firstChild.classList.add('back');
        const key = step + (step === 'keys' ? keyIndex : '');
        const same = root.dataset.step === key;
        root.dataset.step = key;
        keysBox = null;
        const body = h('div', { class: 'onb-body' }, h('div', { class: 'onb-step' + (same ? ' still' : '') }, VIEWS[step]()));
        root.replaceChildren(h('div', { class: 'onb-top' }, h('img', { src: '../assets/logo.svg', alt: '' }), h('b', null, 'Murmint')), body, foot);
      }

      draw();
    });
  }

  window.Onboarding = { run };
})();
