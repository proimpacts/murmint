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

      let user = info.user, local = info.local;
      const saved = App.cfg().onboarding || {};
      let heard = saved.heardFrom || '';
      let signing = null, keyIndex = 0;

      const steps = ['signin', 'heard', 'speech', 'keys', 'done'];
      let step = !user ? 'signin' : !heard ? 'heard' : !local.installed ? 'speech' : 'keys';

      const offs = [
        api.on('state:changed', (st) => { App.S = st; }),
        api.on('local:progress', (p) => { local = { ...local, ...p, installed: p.state === 'installed' || local.installed }; if (step === 'speech') draw(); }),
      ];
      // reaching the last step turns Murmint on, so the shortcut works in the box there before leaving
      const go = (to) => { recorder.stop(); step = to; if (to === 'done') api.finishOnboarding(); draw(); };
      const next = () => go(steps[steps.indexOf(step) + 1]);
      const back = () => {
        if (step === 'keys' && keyIndex > 0) { keyIndex--; recorder.stop(); draw(); return; }
        if (step === 'done') keyIndex = KEYS.length - 1;
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
          if (!local.installed && local.state !== 'downloading' && local.state !== 'error') {
            api.downloadModels().then((st) => { local = { ...local, ...st }; draw(); });
            local = { ...local, state: 'downloading' };
          }
          const pct = local.total ? Math.min(100, Math.round(((local.received || 0) / local.total) * 100)) : 0;
          let status;
          if (local.installed) status = h('div', { class: 'onb-wait good' }, window.icon('check'), 'Murmint 1 is ready');
          else if (local.state === 'error') {
            status = h('div', { class: 'onb-progress' }, h('div', { class: 'onb-err' }, local.error),
              h('div', { class: 'flex', style: { justifyContent: 'center' } }, UI.btn('Try again', { variant: 'primary', onClick: async () => { local = { ...local, ...(await api.downloadModels()) }; draw(); } })));
          } else {
            status = h('div', { class: 'onb-progress' },
              h('div', { class: 'onb-bar' }, h('i', { style: { width: pct + '%' } })),
              h('div', { class: 'onb-dl-meta' }, h('span', null, local.checking ? 'Checking the download…' : `${size(local.received || 0)} of ${size(local.size || local.total)}`), h('span', null, pct + '%')));
          }
          return [head('Getting Murmint 1', 'Murmint’s own speech model runs on this computer, so turning your voice into text is free and private.'), status];
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

        done() {
          const keys = App.cfg().hotkeys.dictate;
          const hold = App.cfg().dictation.trigger !== 'toggle';
          const hk = App.cfg().hotkeys;
          const tile = (combo, title, sub) => h('div', { class: 'onb-sc' }, UI.kbds(combo), h('b', null, title), h('span', null, sub));
          return [
            h('div', { class: 'onb-done-ic' }, window.icon('check')),
            head('You’re all set', 'Murmint 1 handles speech. Add a provider on the Models page and Murmint formats with the best and cheapest model you have.'),
            // a box to try the shortcut in before leaving
            h('label', { class: 'onb-tryit' }, h('span', null, 'Try it here'),
              h('textarea', { rows: 3, spellcheck: false, placeholder: hold ? `Click here, hold ${keys}, say something, then let go.` : `Click here, press ${keys}, say something, then press it again.` })),
            h('div', { class: 'onb-scs' },
              tile(keys, 'Dictate', hold ? 'Hold, speak, let go' : 'Press to start and stop'),
              tile(hk.handsFree, 'Hands-free', 'Keeps listening'),
              tile(hk.formatSelection, 'Format text', 'Cleans up what you select')),
          ];
        },
      };

      function canContinue() {
        if (step === 'signin') return !!user;
        if (step === 'heard') return !!heard;
        if (step === 'speech') return !!local.installed;
        return true;
      }

      async function onContinue(btn) {
        if (step === 'keys' && keyIndex < KEYS.length - 1) { recorder.stop(); keyIndex++; draw(); return; }
        if (step !== 'done') return next();
        btn.disabled = true;
        await api.finishOnboarding();
        offs.forEach((off) => off());
        root.hidden = true; root.replaceChildren(); app.hidden = false;
        finish({ page: 'models' });
      }

      function draw() {
        const i = steps.indexOf(step);
        const cont = UI.btn(step === 'done' ? 'Start using Murmint' : 'Continue', { variant: 'primary', onClick: () => onContinue(cont) });
        cont.classList.add('next');
        cont.disabled = !canContinue();
        const canBack = i > 0 && !(i === 1 && user) && !signing && step !== 'speech';
        const foot = h('footer', { class: 'onb-foot' },
          canBack ? UI.btn('Back', { variant: 'ghost', onClick: back }) : h('span'),
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
