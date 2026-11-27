(() => {
  const api = window.murmintIpc;
  const layer = document.getElementById('layer');
  const close = () => api.closeTranslation();
  const make = (tag, cls, text) => { const e = document.createElement(tag); e.className = cls; if (text) e.textContent = text; return e; };

  // Puts the bubble under the text (or over it when there's no room below), with its tail pointing at it
  function place(bubble, tail, at) {
    const gap = 14, margin = 12, w = bubble.offsetWidth, h = bubble.offsetHeight;
    const below = at.y + at.h + gap, above = at.y - gap - h;
    let y, side;
    if (below + h <= innerHeight - margin) { y = below; side = 'up'; }
    else if (above >= margin) { y = above; side = 'down'; }
    else { y = Math.max(margin, Math.min(below, innerHeight - margin - h)); side = ''; }
    const cx = at.x + at.w / 2;
    const x = Math.max(margin, Math.min(cx - w / 2, innerWidth - w - margin));
    bubble.style.left = x + 'px';
    bubble.style.top = y + 'px';
    tail.className = 'tail ' + side;
    tail.hidden = !side;
    tail.style.left = Math.max(20, Math.min(cx - x, w - 20)) - 9 + 'px';
    bubble.style.transformOrigin = `${cx - x}px ${side === 'down' ? h : 0}px`;
  }

  function show({ target, text, at, theme, accent, truncated }) {
    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.accent = accent || 'ember';
    const bubble = make('div', 'bubble'), tail = make('i', 'tail');
    const copy = make('button', 'btn', 'Copy'), dismiss = make('button', 'btn ghost', 'Dismiss');
    copy.type = dismiss.type = 'button';
    copy.addEventListener('click', async () => {
      let ok = false;
      try { ok = (await api.copyText(text)) === true; } catch { ok = false; }
      copy.textContent = ok ? 'Copied' : 'Clipboard busy — try again';
      setTimeout(() => { copy.textContent = 'Copy'; }, ok ? 1400 : 1800);
    });
    dismiss.addEventListener('click', close);
    const acts = make('div', 'acts');
    acts.append(copy, dismiss);
    const head = make('div', 'head');
    const heading = truncated ? `Translated to ${target} · Long selection shortened` : `Translated to ${target}`;
    head.append(make('span', 'label', heading), acts);
    bubble.append(head, make('div', 'text', text), tail);
    layer.append(bubble);
    place(bubble, tail, at);
  }

  layer.addEventListener('mousedown', (e) => { if (e.target === layer) close(); });
  addEventListener('wheel', (e) => { if (!e.target.closest('.text')) close(); }, { passive: true });
  addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
  api.on('translate:bubble', show);
})();
