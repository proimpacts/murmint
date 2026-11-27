(function () {
  const { h, icon } = UI;
  const api = window.murmint;
  let query = '';
  let extra = [];
  let extraHasMore = false;
  let searchRes = null; // { query, items, hasMore }
  let lastSig = '';
  let reqGen = 0;
  let busy = false;
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
      setTimeout(() => {
        extra = extra.filter((x) => x.id !== e.id);
        if (searchRes) searchRes = { ...searchRes, items: searchRes.items.filter((x) => x.id !== e.id) };
        api.deleteHistory(e.id);
      }, 220);
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
      const hist = App.S.history || [];
      const totalCount = App.S.historyTotal ?? hist.length;
      const totalWords = App.S.historyWords ?? hist.reduce((n, e) => n + (e.words || 0), 0);
      const sig = `${totalCount}:${hist.length}:${hist[0]?.id || ''}:${hist.at(-1)?.id || ''}`;
      const sigChanged = sig !== lastSig;
      if (sigChanged) {
        lastSig = sig;
        if (totalCount === 0) {
          extra = [];
          extraHasMore = false;
          searchRes = null;
        } else {
          if (extra.length) {
            const seen = new Set(hist.map((x) => x.id));
            extra = extra.filter((x) => !seen.has(x.id));
          }
          if (totalCount <= hist.length) { extra = []; extraHasMore = false; }
        }
      }

      const search = h('label', { class: 'hsearch' }, icon('search'));
      const input = h('input', { placeholder: 'Search history', value: query });
      search.append(input);
      root.append(h('div', { class: 'hhead' }, h('h1', null, 'History'), search,
        totalCount ? UI.btn('', { icon: 'trash', variant: 'icon', title: 'Delete all saved dictations', onClick: () => UI.confirm('Delete all saved dictations?').then((ok) => { if (ok) api.clearHistory(); }) }) : ''));
      root.append(h('p', { class: 'muted', style: { margin: '-12px 0 20px' } }, `${totalCount} dictation${totalCount === 1 ? '' : 's'} · ${new Intl.NumberFormat().format(totalWords)} words, kept only on this PC.`));
      if (!App.cfg().privacy.saveHistory) {
        root.append(h('div', { class: 'banner', style: { marginBottom: '16px' } }, icon('lock'), h('div', { class: 'grow' }, h('div', { class: 't' }, 'History is turned off'),
          h('div', { class: 'd' }, 'New dictations are not being saved. You can change this in Settings > Privacy.'))));
      }

      const list = h('div', { class: 'hlist hbox' });
      const foot = h('div', { class: 'flex', style: { justifyContent: 'center', marginTop: '14px' } });

      const currentItems = () => {
        const q = query.trim();
        if (q) {
          if (searchRes && searchRes.query === q) return searchRes.items;
          const ql = q.toLowerCase();
          return hist.filter((e) => `${e.text} ${e.edited || ''} ${e.app}`.toLowerCase().includes(ql));
        }
        return extra.length ? [...hist, ...extra] : hist;
      };

      const currentHasMore = () => {
        const q = query.trim();
        if (q) return searchRes && searchRes.query === q ? searchRes.hasMore : false;
        if (extra.length) return extraHasMore;
        return totalCount > hist.length;
      };

      const draw = () => {
        const q = query.trim();
        const items = currentItems();
        if (!items.length) {
          list.replaceChildren(UI.empty('history', q ? (busy ? 'Searching…' : 'Nothing matches that.') : 'Nothing here yet. Dictate something!'));
        } else {
          const out = [];
          let day = '';
          for (const e of items) {
            const d = dayLabel(e.ts);
            if (d !== day) { day = d; out.push(h('div', { class: 'hday' }, d)); }
            out.push(entry(e));
          }
          list.replaceChildren(...out);
        }
        if (currentHasMore()) {
          const moreBtn = UI.btn(busy ? 'Loading…' : 'Load more', { variant: 'ghost', onClick: loadMore });
          if (busy) moreBtn.disabled = true;
          foot.replaceChildren(moreBtn);
        } else {
          foot.replaceChildren();
        }
      };

      const runQuery = async (q, offset, append) => {
        if (typeof api.queryHistory !== 'function') return;
        const myGen = ++reqGen;
        busy = true;
        draw();
        try {
          const r = await api.queryHistory({ query: q, offset, limit: 100 });
          if (myGen !== reqGen || !list.isConnected) return;
          const incoming = Array.isArray(r?.items) ? r.items : [];
          if (q) {
            const base = append && searchRes && searchRes.query === q ? searchRes.items : [];
            const seen = new Set(base.map((x) => x.id));
            const merged = [...base, ...incoming.filter((x) => !seen.has(x.id))];
            searchRes = { query: q, items: merged, hasMore: !!r?.hasMore };
          } else {
            const seen = new Set([...hist, ...extra].map((x) => x.id));
            extra = [...extra, ...incoming.filter((x) => !seen.has(x.id))];
            extraHasMore = !!r?.hasMore;
          }
        } catch {
          // keep current view if query fails
        } finally {
          if (myGen === reqGen) {
            busy = false;
            if (list.isConnected) draw();
          }
        }
      };

      const loadMore = () => {
        if (busy) return;
        const q = query.trim();
        const offset = q ? (searchRes?.items.length || 0) : currentItems().length;
        runQuery(q, offset, true);
      };

      input.addEventListener('input', () => {
        query = input.value;
        const q = query.trim();
        if (!q) {
          ++reqGen;
          busy = false;
          searchRes = null;
          draw();
          return;
        }
        runQuery(q, 0, false);
      });

      draw();
      root.append(list, foot);
      if (query.trim() && (sigChanged || !searchRes || searchRes.query !== query.trim())) {
        runQuery(query.trim(), 0, false);
      }
    },
  };
})();
