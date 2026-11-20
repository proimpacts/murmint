(function () {
  const h = (tag, props, ...kids) => {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(props || {})) {
      if (v == null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k === 'text') el.textContent = v;
      else if (k === 'html') el.innerHTML = v;
      else if (k === 'on') for (const [ev, fn] of Object.entries(v)) el.addEventListener(ev, fn);
      else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
      else if (k === 'value' || k === 'checked' || k === 'disabled' || k === 'selected') el[k] = v;
      else el.setAttribute(k, v === true ? '' : v);
    }
    const add = (k) => {
      if (k == null || k === false) return;
      if (Array.isArray(k)) k.forEach(add);
      else el.append(k.nodeType ? k : document.createTextNode(String(k)));
    };
    kids.forEach(add);
    return el;
  };

  const debounce = (fn, ms = 350) => { let t; return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); }; };

  const UI = {
    h, debounce,
    icon: (n) => window.icon(n),
    btn(label, { icon, variant = '', onClick, title, size = '' } = {}) {
      const b = h('button', { class: `btn ${variant} ${size}`.trim(), title, 'aria-label': title || (label || undefined) }, icon && window.icon(icon), label);
      if (onClick) b.addEventListener('click', onClick);
      return b;
    },
    toggle(checked, onChange, label) {
      const b = h('button', { class: 'switch', role: 'switch', 'aria-checked': String(!!checked), 'aria-label': label });
      b.addEventListener('click', () => { const v = b.getAttribute('aria-checked') !== 'true'; b.setAttribute('aria-checked', String(v)); onChange(v); });
      return b;
    },
    seg(options, value, onChange) {
      const wrap = h('div', { class: 'seg', role: 'group' });
      const pill = h('span', { class: 'seg-pill', 'aria-hidden': 'true' });
      wrap.append(pill);
      const place = (b, animate) => {
        if (!b || !b.offsetWidth) return;
        if (!animate) pill.style.transition = 'none';
        pill.style.width = b.offsetWidth + 'px';
        pill.style.transform = `translateX(${b.offsetLeft - 3}px)`;
        wrap.classList.add('has-pill');
        if (!animate) { void pill.offsetWidth; pill.style.transition = ''; }
      };
      requestAnimationFrame(() => place(wrap.querySelector('button[aria-pressed="true"]'), false));
      for (const o of options) {
        const b = h('button', { 'aria-pressed': String(o.value === value), title: o.title }, o.label);
        b.addEventListener('click', () => {
          wrap.querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', 'false'));
          b.setAttribute('aria-pressed', 'true'); place(b, true); onChange(o.value);
        });
        wrap.append(b);
      }
      return wrap;
    },
    select(options, value, onChange, cls = '') {
      const flat = [];
      for (const o of options) { if (o.group) o.options.forEach((x) => flat.push({ ...x, group: o.group })); else flat.push(o); }
      let cur = value;
      const label = h('span', { class: 'dd-label' });
      const btn = h('button', { type: 'button', class: 'select dd ' + cls, 'aria-haspopup': 'listbox', 'aria-expanded': 'false' }, label, h('span', { class: 'dd-caret' }));
      const paint = () => { const f = flat.find((x) => x.value === cur); label.textContent = f ? f.label : ''; label.classList.toggle('dd-placeholder', !f || f.value === ''); };
      paint();
      Object.defineProperty(btn, 'value', { get: () => cur, set: (v) => { cur = v; paint(); } });
      let menu = null, idx = -1;
      const away = (e) => { if (menu && !menu.contains(e.target) && !btn.contains(e.target)) close(); };
      const close = () => { if (!menu) return; menu.remove(); menu = null; btn.setAttribute('aria-expanded', 'false'); document.removeEventListener('mousedown', away, true); window.removeEventListener('resize', close); document.removeEventListener('scroll', closeOnScroll, true); };
      const closeOnScroll = (e) => { if (menu && !menu.contains(e.target)) close(); };
      const pick = (o) => { if (o.disabled) return; cur = o.value; paint(); close(); onChange(o.value); };
      const mark = (i) => { idx = i; [...menu.querySelectorAll('.dd-item')].forEach((el, n) => { el.classList.toggle('hot', n === i); if (n === i) el.scrollIntoView({ block: 'nearest' }); }); };
      const open = () => {
        if (menu) return close();
        menu = h('div', { class: 'dd-menu', role: 'listbox' });
        let lastGroup = null, n = 0;
        for (const o of flat) {
          if (o.group && o.group !== lastGroup) { menu.append(h('div', { class: 'dd-group' }, o.group)); lastGroup = o.group; }
          const i = n++;
          menu.append(h('div', { class: 'dd-item' + (o.value === cur ? ' on' : '') + (o.disabled ? ' off' : ''), role: 'option', 'aria-selected': String(o.value === cur), on: { mousedown: (e) => { e.preventDefault(); pick(o); }, mousemove: () => mark(i) } }, h('span', { class: 'dd-text' }, o.label), o.value === cur ? h('span', { class: 'dd-check' }) : null));
        }
        document.body.append(menu);
        const r = btn.getBoundingClientRect();
        const below = window.innerHeight - r.bottom, want = Math.min(menu.scrollHeight, 320);
        const up = below < want + 16 && r.top > below;
        menu.style.minWidth = r.width + 'px'; menu.style.maxHeight = Math.max(140, Math.min(320, (up ? r.top : below) - 16)) + 'px';
        menu.style.left = Math.max(8, Math.min(r.left, window.innerWidth - menu.offsetWidth - 8)) + 'px';
        if (up) { menu.style.bottom = (window.innerHeight - r.top + 6) + 'px'; menu.classList.add('up'); } else menu.style.top = (r.bottom + 6) + 'px';
        btn.setAttribute('aria-expanded', 'true');
        document.addEventListener('mousedown', away, true); window.addEventListener('resize', close); document.addEventListener('scroll', closeOnScroll, true);
        mark(Math.max(0, flat.findIndex((x) => x.value === cur)));
      };
      btn.addEventListener('click', open);
      btn.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') { close(); return; }
        if (!['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(e.key)) return;
        e.preventDefault();
        if (!menu) { open(); return; }
        if (e.key === 'ArrowDown') mark(Math.min(flat.length - 1, idx + 1));
        else if (e.key === 'ArrowUp') mark(Math.max(0, idx - 1));
        else if (flat[idx]) pick(flat[idx]);
      });
      return btn;
    },
    input({ value = '', placeholder, type = 'text', onInput, onChange, cls = '', mono, autocomplete = 'off' } = {}) {
      const i = h('input', { class: `input ${mono ? 'mono' : ''} ${cls}`, type, placeholder, value, autocomplete, spellcheck: 'false' });
      if (onInput) i.addEventListener('input', () => onInput(i.value));
      if (onChange) i.addEventListener('change', () => onChange(i.value));
      return i;
    },
    textarea({ value = '', placeholder, rows = 3, onInput, onChange, cls = '', mono } = {}) {
      const t = h('textarea', { class: `textarea ${mono ? 'mono' : ''} ${cls}`, placeholder, rows, spellcheck: 'false' });
      t.value = value;
      if (onInput) t.addEventListener('input', () => onInput(t.value));
      if (onChange) t.addEventListener('change', () => onChange(t.value));
      return t;
    },
    row(title, desc, control, { stack } = {}) {
      return h('div', { class: `row ${stack ? 'stack' : ''}` }, h('div', { class: 'grow' }, h('div', { class: 't' }, title), desc && h('div', { class: 'd' }, desc)), control && h('div', { class: 'ctl' }, control));
    },
    card: (...kids) => h('div', { class: 'card' }, ...kids),
    section(title, desc, ...kids) {
      return h('section', { class: 'section' }, h('h2', null, title), desc && h('p', { class: 'desc' }, desc), ...kids);
    },
    pageHead: (title, sub, eyebrow) => h('div', { class: 'page-head' }, eyebrow && h('span', { class: 'eyebrow' }, eyebrow), h('h1', null, title), sub && h('p', null, sub)),
    loader: (label) => h('span', { class: 'ldr', role: 'status', 'aria-label': label || 'Working' }, h('i'), h('i'), h('i')),
    badge: (text, kind = '') => h('span', { class: `badge ${kind}` }, text),
    chip(text, onRemove) {
      return h('span', { class: 'chip' }, text, onRemove && h('button', { title: 'Remove', 'aria-label': `Remove ${text}`, on: { click: onRemove } }, window.icon('x')));
    },
    kbds(combo, large) {
      const parts = (combo || '').split('+').filter(Boolean);
      const out = h('span', { class: 'kbds' });
      parts.forEach((p, i) => { if (i) out.append(h('i', null, '+')); out.append(h('span', { class: large ? 'kbd lg' : 'kbd' }, p === 'Escape' ? 'Esc' : p)); });
      return out;
    },
    empty(icon, text) { return h('div', { class: 'empty' }, window.icon(icon), h('div', null, text)); },
    toast(message, kind = 'info') {
      const box = document.getElementById('toasts');
      const t = h('div', { class: `toast ${kind}`, role: 'status' }, window.icon(kind === 'error' ? 'alert' : kind === 'ok' ? 'check' : 'info'), h('div', null, message));
      box.append(t);
      setTimeout(() => { t.style.transition = 'opacity .25s'; t.style.opacity = '0'; setTimeout(() => t.remove(), 260); }, kind === 'error' ? 6500 : 3200);
    },
    modal(content) {
      const bg = h('div', { class: 'modal-bg' }, h('div', { class: 'modal', role: 'dialog' }, content));
      const close = () => bg.remove();
      bg.addEventListener('mousedown', (e) => { if (e.target === bg) close(); });
      document.addEventListener('keydown', function esc(e) { if (e.key === 'Escape') { close(); document.removeEventListener('keydown', esc); } });
      document.body.append(bg);
      return close;
    },
    confirm(message, okLabel = 'Delete') {
      return new Promise((resolve) => {
        let done = false;
        const fin = (v) => { if (done) return; done = true; close(); resolve(v); };
        const close = UI.modal(h('div', null, h('h3', null, message), h('div', { class: 'flex mt3', style: { justifyContent: 'flex-end' } },
          UI.btn('Cancel', { variant: 'ghost', onClick: () => fin(false) }), UI.btn(okLabel, { variant: 'danger', onClick: () => fin(true) }))));
        document.querySelector('.modal-bg:last-of-type')?.addEventListener('mousedown', (e) => { if (e.target.classList.contains('modal-bg')) fin(false); });
      });
    },
    money(n) {
      if (!n) return '$0.00';
      if (n < 0.01) return `$${n.toFixed(4)}`;
      return `$${n.toFixed(n < 10 ? 3 : 2)}`;
    },
    ago(ts) {
      const s = Math.max(0, (Date.now() - ts) / 1000);
      if (s < 60) return 'just now';
      if (s < 3600) return `${Math.floor(s / 60)} min ago`;
      if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
      const d = new Date(ts);
      return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) + ' ' + d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
    },
    uid: () => (crypto.randomUUID ? crypto.randomUUID() : String(Math.random()).slice(2)),
  };
  window.UI = UI;
})();
