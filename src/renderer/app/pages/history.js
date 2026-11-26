(function () {
  const { h, icon } = UI;
  const api = window.murmint;
  let query = '';
  const open = new Set(); // entries showing what was actually said
  const dayLabel = (ts) => {
    const d = new Date(ts), today = new Date();
    const start = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const days = Math.round((start(today) - start(d)) / 86400000);
    if (days === 0) return 'Today';
    if (days === 1) return 'Yesterday';
    return d.toLocaleDateString([], { weekday: 'long', month: 'long', day: 'numeric' });
  };
  const time = (ts) => new Date(ts).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  // the date and the time, in this PC's time zone
  const when = (ts) => {
    const d = new Date(ts);
    const date = d.toLocaleDateString([], { month: 'short', day: 'numeric', ...(d.getFullYear() !== new Date().getFullYear() ? { year: 'numeric' } : {}) });
    return [h('span', { class: 'hdate' }, date), h('span', null, time(ts))];
  };

  function entry(e) {
    const typed = e.source === 'selection'; // formatted with the Format selection shortcut, not dictated
    const text = e.edited || e.text;
    const tile = window.AppTile(e);
    const copy = async (btn) => {
      await api.copyText(text);
      btn.classList.add('done');
      setTimeout(() => btn.classList.remove('done'), 1200);
    };
    const copyBtn = h('button', { class: 'hact', type: 'button', title: 'Copy' }, icon('copy'));
    copyBtn.addEventListener('click', (ev) => { ev.stopPropagation(); copy(copyBtn); });
    const rawBtn = h('button', { class: `hact${open.has(e.id) ? ' on' : ''}`, type: 'button', title: typed ? 'What you typed' : 'What you said' }, icon(typed ? 'type' : 'mic'));
    rawBtn.addEventListener('click', (ev) => { ev.stopPropagation(); open.has(e.id) ? open.delete(e.id) : open.add(e.id); App.render(); });
    const delBtn = h('button', { class: 'hact bad', type: 'button', title: 'Delete' }, icon('trash'));
    delBtn.addEventListener('click', (ev) => {
      ev.stopPropagation();
      row.classList.add('leaving');
      setTimeout(() => api.deleteHistory(e.id), 220);
    });
    const meta = [e.app, `${e.words} word${e.words === 1 ? '' : 's'}`, typed ? 'formatted from typed text' : '', e.edited ? 'edited' : ''].filter(Boolean).join(' · ');
    const row = h('article', { class: 'hrow', title: 'Click to copy' },
      h('time', { datetime: new Date(e.ts).toISOString(), title: new Date(e.ts).toLocaleString() }, when(e.ts)),
      tile,
      h('div', { class: 'hbody' },
        h('div', { class: 'htext' }, text),
        h('div', { class: 'hmeta' }, meta),
        open.has(e.id) && e.raw ? h('div', { class: 'hraw' }, h('span', null, typed ? 'What you typed' : 'What you said'), e.raw) : ''),
      h('div', { class: 'hacts' }, copyBtn, rawBtn, delBtn));
    row.addEventListener('click', () => { if (!window.getSelection().toString()) copy(copyBtn).then(() => UI.toast('Copied', 'ok')); });
    return row;
  }

  Pages.history = {
    render(root, App) {
      const hist = App.S.history;
      const words = hist.reduce((n, e) => n + (e.words || 0), 0);
      const search = h('label', { class: 'hsearch' }, icon('search'));
      const input = h('input', { placeholder: 'Search history', value: query });
      search.append(input);
      root.append(h('div', { class: 'hhead' }, h('h1', null, 'History'), search,
        hist.length ? UI.btn('', { icon: 'trash', variant: 'icon', title: 'Delete all saved dictations', onClick: () => UI.confirm('Delete all saved dictations?').then((ok) => { if (ok) api.clearHistory(); }) }) : ''));
      root.append(h('p', { class: 'muted', style: { margin: '-12px 0 20px' } }, `${hist.length} dictation${hist.length === 1 ? '' : 's'} · ${new Intl.NumberFormat().format(words)} words, kept only on this PC.`));
      if (!App.cfg().privacy.saveHistory) {
        root.append(h('div', { class: 'banner', style: { marginBottom: '16px' } }, icon('lock'), h('div', { class: 'grow' }, h('div', { class: 't' }, 'History is turned off'),
          h('div', { class: 'd' }, 'New dictations are not being saved. You can change this in Settings > Privacy.'))));
      }

      const list = h('div', { class: 'hlist hbox' });
      const draw = () => {
        const q = query.trim().toLowerCase();
        const items = hist.filter((e) => !q || `${e.text} ${e.edited || ''} ${e.app}`.toLowerCase().includes(q));
        if (!items.length) { list.replaceChildren(UI.empty('history', q ? 'Nothing matches that.' : 'Nothing here yet. Dictate something!')); return; }
        const out = [];
        let day = '';
        for (const e of items) {
          const d = dayLabel(e.ts);
          if (d !== day) { day = d; out.push(h('div', { class: 'hday' }, d)); }
          out.push(entry(e));
        }
        list.replaceChildren(...out);
      };
      input.addEventListener('input', () => { query = input.value; draw(); });
      draw();
      root.append(list);
    },
  };
})();
