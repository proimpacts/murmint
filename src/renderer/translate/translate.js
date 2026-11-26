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

  function draw({ target, card, blocks, cursor }) {
    let top = Infinity, left = 0;
    if (card) {
      const el = document.createElement('div');
      el.className = 'card';
      el.textContent = blocks.map((b) => b.text).join('\n\n');
      layer.append(el);
      const x = Math.min(cursor.x + 12, innerWidth - el.offsetWidth - 12), y = Math.min(cursor.y + 16, innerHeight - el.offsetHeight - 12);
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
    chip.textContent = `Translated to ${target} · Esc to close`;
    chip.style.left = Math.max(8, left) + 'px';
    chip.style.top = (top > 34 ? top - 28 : top + 4) + 'px';
    if (top <= 34 && !card) chip.style.top = '8px';
  }

  layer.addEventListener('mousedown', (e) => { if (e.target === layer) close(); });
  addEventListener('wheel', close, { passive: true });
  addEventListener('keydown', (e) => { if (e.key === 'Escape') close(); });
  api.on('translate:blocks', draw);
})();
