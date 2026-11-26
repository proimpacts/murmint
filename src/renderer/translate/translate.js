(() => {
  const api = window.murmintIpc;
  const layer = document.getElementById('layer'), chip = document.getElementById('chip');
  const close = () => api.closeTranslation();

  // Shrinks the type until the translation fits the original's box; if it still doesn't, the box grows downward
  function fit(el, b) {
    let size = Math.max(10, Math.min(28, b.line * 0.74));
    el.style.fontSize = size + 'px';
    while (el.scrollHeight > b.h + 1 && size > 10) { size -= 0.5; el.style.fontSize = size + 'px'; }
    if (el.scrollHeight > b.h + 1) { el.style.height = 'auto'; el.classList.add('grown'); }
  }

  function copyButton(text) {
    const copy = document.createElement('button');
    copy.type = 'button';
    copy.className = 'copy';
    copy.textContent = 'Copy';
    copy.addEventListener('mousedown', (e) => e.stopPropagation());
    copy.addEventListener('click', async () => {
      await api.copyText(text);
      copy.textContent = 'Copied';
      copy.classList.add('done');
      setTimeout(() => { copy.textContent = 'Copy'; copy.classList.remove('done'); }, 1400);
    });
    return copy;
  }

  function draw({ target, card, blocks, cursor, anchor }) {
    let top = Infinity, left = 0;
    if (card) {
      const text = blocks.map((b) => b.text).join('\n\n');
      const el = document.createElement('div');
      el.className = 'card';
      const head = document.createElement('div');
      head.className = 'card-head';
      const label = document.createElement('span');
      label.textContent = `Translated to ${target}`;
      head.append(label, copyButton(text));
      const body = document.createElement('div');
      body.className = 'card-text';
      body.textContent = text;
      el.append(head, body);
      layer.append(el);
      let x, y;
      // a whole page or editor is too big to sit under; then the card goes where you were pointing
      if (anchor && anchor.h < 260) {
        // just under the box the text came from, or over its top when there's no room below
        x = anchor.x;
        y = anchor.y + anchor.h + 8;
        if (y + el.offsetHeight > innerHeight - 12) y = Math.max(12, anchor.y + 8);
      } else {
        x = cursor.x + 12; y = cursor.y + 16;
      }
      x = Math.min(x, innerWidth - el.offsetWidth - 12); y = Math.min(y, innerHeight - el.offsetHeight - 12);
      el.style.left = Math.max(12, x) + 'px'; el.style.top = Math.max(12, y) + 'px';
      top = el.offsetTop; left = el.offsetLeft;
    } else {
      for (const b of blocks) {
        if (!b.w) continue;
        const el = document.createElement('p');
        el.className = 'tb';
        el.textContent = b.text;
        el.title = b.original;
        Object.assign(el.style, { left: b.x + 'px', top: b.y + 'px', width: b.w + 'px', height: b.h + 'px', background: b.bg || '#fff', color: b.fg || '#111' });
        el.addEventListener('mousedown', (e) => { if (e.button === 0) el.classList.add('peek'); });
        el.addEventListener('mouseup', () => el.classList.remove('peek'));
        el.addEventListener('mouseleave', () => el.classList.remove('peek'));
        layer.append(el);
        fit(el, b);
        if (b.y < top) { top = b.y; left = b.x; }
      }
    }
    // the card's own header names the language, so the chip is only for the in-place view
    chip.hidden = !!card;
    chip.textContent = `Translated to ${target} · Esc to close`;
    if (!card) chip.append(copyButton(blocks.filter((b) => b.w).map((b) => b.text).join('\n\n')));
    chip.style.left = Math.max(8, left) + 'px';
    chip.style.top = (top > 34 ? top - 28 : top + 4) + 'px';
    if (top <= 34 && !card) chip.style.top = '8px';
  }

  layer.addEventListener('mousedown', (e) => { if (e.target === layer) close(); });
  addEventListener('wheel', close, { passive: true });
  addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
  api.on('translate:blocks', draw);
})();
