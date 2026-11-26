(function () {
  const { h, icon } = UI;
  const api = window.murmint;

  const App = {
    S: null,
    page: 'dashboard',
    Pages: {},
    groups: [
      [null, [['dashboard', 'Home', 'chart'], ['history', 'History', 'clipboard']]],
      ['Voice', [['models', 'Models', 'models'], ['styles', 'Writing', 'textsize'], ['apps', 'Apps', 'apps'], ['vocabulary', 'Vocabulary', 'mic']]],
    ],
    footer: [[null, 'Community', 'users', 'https://discord.gg/MEfdzfyARV'], [null, 'Help', 'help', 'https://murmint.app/faq'], ['settings', 'Settings', 'gear']],

    cfg: () => App.S.config,

    set(path, value, { render = false } = {}) {
      const parts = path.split('.');
      let o = App.S.config;
      for (let i = 0; i < parts.length - 1; i++) o = o[parts[i]] ??= {};
      o[parts.at(-1)] = value;
      api.set(path, value);
      App._ownAt = Date.now();
      if (path.startsWith('appearance.')) App.applyAppearance(); // recolour at once, not when the saved state comes back
      // keep sidebar and Settings theme switches in sync
      if (path === 'appearance.theme') { App.renderSidebar(); if (!render) App.render(); }
      if (render) App.render();
    },

    modelOptions(task, { allowEmpty = false } = {}) {
      const groups = [];
      for (const p of App.cfg().providers) {
        const models = p.models.filter((m) => m.task === task);
        if (!models.length) continue;
        const ready = App.S.providerStatus[p.id]?.ready;
        groups.push({ group: `${p.name}${ready ? '' : ' (not set up)'}`, options: models.map((m) => ({ value: `${p.id}::${m.id}`, label: m.label })) });
      }
      return allowEmpty ? [{ value: '', label: 'None' }, ...groups] : groups;
    },
    refLabel(ref) {
      if (!ref) return 'None';
      const i = ref.indexOf('::');
      const p = App.cfg().providers.find((x) => x.id === ref.slice(0, i));
      const m = p && p.models.find((x) => x.id === ref.slice(i + 2));
      return m ? `${m.label}` : ref;
    },
    providerOfRef(ref) { return App.cfg().providers.find((x) => x.id === (ref || '').split('::')[0]); },
    anyReady: () => Object.values(App.S.providerStatus).some((s) => s.ready),

    // suppress transitions for one frame to avoid a colour flash
    _quiet(el) {
      el.classList.add('no-trans');
      requestAnimationFrame(() => requestAnimationFrame(() => el.classList.remove('no-trans')));
    },

    nav(page) {
      const next = App.Pages[page] ? page : 'dashboard';
      const changed = next !== App.page;
      App.page = next;
      App.renderSidebar();
      const calm = document.documentElement.dataset.motion === 'reduced' || matchMedia('(prefers-reduced-motion: reduce)').matches;
      if (!changed || calm) { App.render(); return; }
      const main = document.getElementById('main');
      const loader = UI.h('div', { class: 'page-loader', 'aria-label': 'Loading' }, ...Array.from({ length: 5 }, () => UI.h('i')));
      main.replaceChildren(loader);
      clearTimeout(App._navTimer);
      App._navTimer = setTimeout(() => { if (App.page === next) App.render(); }, 340);
    },

    renderSidebar() {
      const sb = document.getElementById('sidebar');
      App._quiet(sb);
      sb.innerHTML = '';
      const navBtn = ([id, label, ic]) => h('button', { class: `nav ${App.page === id ? 'active' : ''}`, 'aria-current': App.page === id ? 'page' : undefined, on: { click: () => App.nav(id) } }, window.icon(App.page === id ? `${ic}:on` : ic), label);
      App.groups.forEach(([, items], i) => { if (i) sb.append(h('div', { class: 'nav-sep' })); items.forEach((it) => sb.append(navBtn(it))); });
      sb.append(h('div', { class: 'nav-spacer' }));
      // the foot: small icon buttons, as in SayIt, and the theme switch
      const foot = h('div', { class: 'side-foot' }, ...App.footer.map(([id, label, ic, url]) => url
        ? h('a', { class: 'ibtn', href: url, target: '_blank', rel: 'noopener', title: label, 'aria-label': label }, window.icon(ic))
        : h('button', { class: `ibtn ${App.page === id ? 'active' : ''}`, type: 'button', title: label, 'aria-label': label, on: { click: () => App.nav(id) } }, window.icon(ic))));
      const theme = UI.seg([{ value: 'system', label: 'Auto' }, { value: 'light', label: 'Light' }, { value: 'dark', label: 'Dark' }],
        App.cfg().appearance.theme, (v) => App.set('appearance.theme', v));
      theme.classList.add('side-theme');
      foot.append(theme);
      sb.append(foot);
      App.renderTitlebar();
      if (window.Motion) Motion.navGlow(sb);
    },

    // The bar across the top: the name on the left, whether dictation is ready on the right
    renderTitlebar() {
      const ready = App.anyReady();
      document.getElementById('titlebar').replaceChildren(
        h('img', { src: '../assets/logo.svg', alt: '' }), h('b', null, 'Murmint'),
        h('div', { class: 'tb-right' }, h('button', { class: 'tb-status', type: 'button', title: ready ? `Hold ${App.cfg().hotkeys.dictate} anywhere` : 'Open Models to set one up', on: { click: () => App.nav('models') } },
          h('span', { class: `dot ${ready ? 'good' : 'warn'}` }), ready ? 'Ready to dictate' : 'No AI provider yet')));
    },

    render() {
      const main = document.getElementById('main');
      const page = App.Pages[App.page];
      const top = main.scrollTop, oldH = main.scrollHeight;
      const root = h('div', { class: 'page', id: 'page' });
      page.render(root, App);
      const sig = App.page + '|' + ((root.querySelector('.tab[aria-selected="true"]') || {}).textContent || '');
      const fresh = sig !== App._sig; App._sig = sig;
      const pageChanged = App.page !== App._pg; App._pg = App.page;
      App._quiet(main);
      main.innerHTML = '';
      main.style.overflowAnchor = 'none';
      main.append(root);
      if (!fresh) { root.style.minHeight = (oldH - 0) + 'px'; setTimeout(() => { if (root.isConnected) root.style.minHeight = ''; }, 2500); }
      main.scrollTop = fresh ? 0 : top;
      if (window.Motion) { const t = root.querySelector('.tabs'); if (t && !pageChanged) Motion.tabLine(t); else { Motion._tab = null; if (t) Motion.tabLine(t); } }
      if (fresh && pageChanged) { if (window.Motion) { Motion.split(root.querySelector('.page-head h1, .home-head h1')); Motion.enter([...root.children].slice(0, 12)); } }
      else if (fresh) root.classList.add('swap');
      else root.style.animation = 'none';
      if (!fresh) requestAnimationFrame(() => { if (Math.abs(main.scrollTop - top) > 1) main.scrollTop = top; });
    },

    isEditing() {
      const a = document.activeElement;
      if (!a || !document.getElementById('main').contains(a)) return false;
      return (a.tagName === 'INPUT' && !['checkbox', 'button', 'range'].includes(a.type)) || a.tagName === 'TEXTAREA' || a.tagName === 'SELECT' || !!document.querySelector('.dd-menu');
    },

    applyAppearance() {
      const a = App.cfg().appearance;
      const theme = a.theme === 'system' ? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : a.theme;
      const r = document.documentElement;
      const changed = r.dataset.theme !== theme || r.dataset.accent !== a.accent;
      if (changed) App._quiet(r);
      r.dataset.theme = theme; r.dataset.accent = a.accent; r.dataset.motion = a.reduceMotion ? 'reduced' : '';
    },

    async onState(st) {
      App.S = st;
      App.applyAppearance(); // the sidebar only changes when you move between pages, so it is not rebuilt here
      const ready = App.anyReady();
      if (ready !== App._ready) { App._ready = ready; App.renderSidebar(); } // the status card at the bottom
      if (App._hold) return; // a page is busy and will redraw itself when it's done
      if (Date.now() - (App._ownAt || 0) < 1500) return;
      if (!App.isEditing() && !document.querySelector('.modal-bg')) App.render();
    },
  };
  window.App = App;
  window.Pages = App.Pages;
})();
