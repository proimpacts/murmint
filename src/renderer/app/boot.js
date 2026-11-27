(async () => {
  const api = window.murmint;
  if (new URLSearchParams(location.search).get('mica') === '1') document.documentElement.classList.add('mica');
  document.getElementById('wc-min').onclick = () => api.win.minimize();
  document.getElementById('wc-max').onclick = () => api.win.toggleMaximize();
  document.getElementById('wc-close').onclick = () => api.win.close();
  App.S = await api.getState();
  App.applyAppearance();
  api.on('gate:changed', () => location.reload());
  const gate = await api.onboarding();
  if (!gate.open) App.page = (await Onboarding.run(gate)).page;
  App.renderSidebar();
  App.render();

  api.on('state:changed', (st) => App.onState(st));
  api.on('session:state', (s) => {
    App.S.session = { state: s.state, mode: s.mode };
    App.renderSidebar();
    if (App.page === 'dashboard' && !App.isEditing()) App.render();
  });
  api.on('toast', (t) => UI.toast(t.message, t.kind));
  api.on('usage:changed', () => { if (App.page === 'dashboard' || App.page === 'models') App.render(); });
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => App.applyAppearance());
  const showUpdate = (u) => {
    if (!u || document.getElementById('update-bar')) return;
    const bar = document.createElement('div'); bar.id = 'update-bar'; bar.className = 'update-bar';
    const label = document.createElement('span'); label.textContent = 'Murmint ' + u.version + ' is available. Would you like to install?';
    const go = document.createElement('button'); go.className = 'btn primary sm'; go.textContent = 'Install';
    const later = document.createElement('button'); later.className = 'btn ghost sm'; later.textContent = 'Later';
    go.onclick = async () => {
      go.disabled = true;
      go.textContent = 'Updating…';
      try {
        const r = await api.installUpdate();
        if (!r || !r.ok) {
          go.disabled = false;
          go.textContent = 'Install';
          UI.toast?.(r?.message || 'Update verification failed', 'error');
        }
      } catch (e) {
        go.disabled = false;
        go.textContent = 'Install';
        UI.toast?.(e?.message || 'Update verification failed', 'error');
      }
    };
    later.onclick = () => bar.remove();
    bar.append(label, go, later); document.body.append(bar);
  };
  api.on('update:available', showUpdate);
  api.getUpdate().then(showUpdate).catch(() => {});
  api.on('nav', (page) => App.nav(page));
  window.addEventListener('contextmenu', (e) => { if (!/INPUT|TEXTAREA/.test(e.target.tagName)) e.preventDefault(); });
})();
