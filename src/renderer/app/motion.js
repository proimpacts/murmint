(function () {
  const calm = () => document.documentElement.dataset.motion === 'reduced' || matchMedia('(prefers-reduced-motion: reduce)').matches;
  const SPRING = 'cubic-bezier(.2, .9, .25, 1.06)';
  const OUT = 'cubic-bezier(.16, 1, .3, 1)';
  let lastY = null;

  const Motion = {
    navGlow(sidebar) {
      const active = sidebar.querySelector('.nav.active');
      if (!active) return;
      const glow = document.createElement('div');
      glow.className = 'nav-glow';
      sidebar.prepend(glow);
      sidebar.classList.add('has-glow');
      const y = active.offsetTop;
      glow.style.height = active.offsetHeight + 'px';
      glow.style.transform = `translateY(${y}px)`;
      if (lastY !== null && lastY !== y && !calm()) glow.animate([{ transform: `translateY(${lastY}px)` }, { transform: `translateY(${y}px)` }], { duration: 280, easing: SPRING });
      lastY = y;
    },

    tabLine(tabs) {
      const active = tabs && tabs.querySelector('.tab[aria-selected="true"]');
      if (!active) return;
      const line = document.createElement('span');
      line.className = 'tab-line';
      tabs.append(line);
      tabs.classList.add('has-line');
      const x = active.offsetLeft, w = active.offsetWidth, prev = Motion._tab;
      Motion._tab = { x, w };
      line.style.width = '100px'; line.style.transformOrigin = '0 0';
      const at = (px, pw) => `translateX(${px}px) scaleX(${pw / 100})`;
      line.style.transform = at(x, w);
      if (prev && !calm() && (prev.x !== x || prev.w !== w)) line.animate([{ transform: at(prev.x, prev.w) }, { transform: at(x, w) }], { duration: 280, easing: SPRING });
    },

    countUp(el, to, format = (x) => String(x)) {
      if (calm() || !Number.isFinite(to) || to <= 0) return;
      if (!el.firstChild) el.append('');
      const t0 = performance.now(), D = 600, node = el.firstChild;
      const step = (now) => {
        const p = Math.min(1, (now - t0) / D), e = 1 - Math.pow(1 - p, 4);
        node.nodeValue = format(Math.round(to * e));
        if (p < 1) requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
    },

    enter(els) {
      if (calm() || !els.length) return;
      els.forEach((el, i) => el.animate(
        [{ opacity: 0, transform: 'translateY(14px)', filter: 'blur(6px)' }, { opacity: 1, transform: 'none', filter: 'blur(0)' }],
        { duration: 560, delay: 60 + i * 55, easing: OUT, fill: 'backwards' },
      ));
    },

    split(el) {
      if (!el || el.dataset.split) return;
      el.dataset.split = '1';
      const words = [];
      const walk = (node) => {
        for (const n of [...node.childNodes]) {
          if (n.nodeType === 3) {
            const frag = document.createDocumentFragment();
            n.nodeValue.split(/(\s+)/).forEach((part) => {
              if (!part) return;
              if (/^\s+$/.test(part)) { frag.append(part); return; }
              const outer = document.createElement('span'); outer.className = 'split-w';
              const inner = document.createElement('span'); inner.textContent = part;
              outer.append(inner); frag.append(outer); words.push(inner);
            });
            n.replaceWith(frag);
          } else if (n.nodeType === 1) walk(n);
        }
      };
      walk(el);
      if (calm()) return;
      words.forEach((w, i) => w.animate(
        [{ transform: 'translateY(105%) rotate(4deg)' }, { transform: 'none' }],
        { duration: 760, delay: 40 + i * 70, easing: OUT, fill: 'backwards' },
      ));
    },

    // idle waveform: summed sines
    wave(box, count = 64) {
      const bars = Array.from({ length: count }, () => document.createElement('i'));
      box.replaceChildren(...bars);
      if (calm()) return;
      const t0 = performance.now();
      const tick = (now) => {
        if (!box.isConnected) return;
        if (!document.hidden) {
          const t = (now - t0) / 1000;
          const intro = Math.min(1, t / 1.2);
          bars.forEach((b, i) => {
            const x = i / count;
            const env = Math.sin(Math.PI * x);
            const v = 0.5 + 0.5 * Math.sin(x * 9 + t * 1.6) * Math.sin(x * 3.3 - t * 0.9) + 0.18 * Math.sin(x * 23 + t * 3.1);
            b.style.transform = `scaleY(${(0.06 + env * Math.abs(v) * 0.9 * intro).toFixed(3)})`;
          });
        }
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    },

  };

  // see .card.spot
  document.addEventListener('pointermove', (e) => {
    const card = e.target.closest && e.target.closest('.card');
    if (!card) return;
    card.classList.add('spot');
    const r = card.getBoundingClientRect();
    card.style.setProperty('--mx', `${e.clientX - r.left}px`);
    card.style.setProperty('--my', `${e.clientY - r.top}px`);
  }, { passive: true });
  window.Motion = Motion;
})();
