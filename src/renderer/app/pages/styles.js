(function () {
  const { h, icon } = UI;
  const api = window.murmint;
  let editId = null;
  const INTENSITY = [
    [0, 'Verbatim', 'Only punctuation and capitalization. Nothing is rewritten and no AI is used.'],
    [1, 'Light', 'Fixes grammar and spelling, removes fillers and repeats. Keeps your wording.'],
    [2, 'Balanced', 'Also tightens clumsy sentences and adds structure, paragraphs, lists, when your words call for it.'],
    [3, 'Strong', 'Rewrites freely for clarity and the chosen style while keeping every fact.'],
  ];
  function styleRow(cfg, st) {
    const active = st.id === cfg.formatting.style;
    const save = UI.debounce((v) => { App.set('styles', App.cfg().styles.map((x) => (x.id === st.id ? { ...x, instruction: v } : x))); });
    const row = h('div', { class: 'row' },
      h('div', { class: 'grow' }, h('div', { class: 't' }, st.name), h('div', { class: 'd' }, st.description || 'Custom style')),
      h('div', { class: 'ctl' },
        UI.btn(editId === st.id ? 'Done' : 'Edit', { variant: 'ghost sm', onClick: () => { editId = editId === st.id ? null : st.id; App.render(); } }),
        UI.toggle(active, (v) => { if (v) App.set('formatting.style', st.id, { render: true }); else App.render(); }, st.name)));
    if (editId !== st.id) return [row];
    const panel = h('div', { class: 'row stack' },
      h('div', { class: 'd' }, 'What the AI is told when this style is on. Edit freely.'),
      UI.textarea({ value: st.instruction, rows: 3, onInput: (v) => { st.instruction = v; save(v); } }),
      !st.builtin ? h('div', null, UI.btn('Delete style', { variant: 'ghost danger sm', onClick: () => { App.set('styles', cfg.styles.filter((x) => x.id !== st.id)); if (cfg.formatting.style === st.id) App.set('formatting.style', 'natural'); editId = null; App.render(); } })) : null);
    return [row, panel];
  }

  function addStyleModal() {
    let close;
    const name = UI.input({ placeholder: 'Name, e.g. “Friendly founder”' });
    const ins = UI.textarea({ placeholder: 'Describe how the text should read. e.g. Warm, upbeat, short sentences, no jargon.', rows: 4 });
    close = UI.modal(h('div', null, h('h3', null, 'New writing style'), h('div', { class: 'grid mt2', style: { gap: '10px' } }, name, ins),
      h('div', { class: 'flex mt3', style: { justifyContent: 'flex-end' } }, UI.btn('Cancel', { variant: 'ghost', onClick: () => close() }),
        UI.btn('Create', { variant: 'primary', onClick: () => {
          if (!name.value.trim()) return;
          const s = { id: 'style-' + UI.uid().slice(0, 8), name: name.value.trim(), description: 'Custom', instruction: ins.value.trim() || 'Write naturally.', builtin: false };
          App.set('styles', [...App.cfg().styles, s]); App.set('formatting.style', s.id); editId = s.id; close(); App.render();
        } }))));
  }

  const setRules = (list, render) => App.set('formatting.rules', list, render ? { render: true } : undefined);

  Pages.styles = {
    render(root, App) {
      const cfg = App.cfg(); const f = cfg.formatting;
      root.append(UI.pageHead('Writing', 'How your speech turns into text.'));

      root.append(UI.section('Writing style', 'Used everywhere, unless an app has its own.',
        UI.card(...cfg.styles.flatMap((st) => styleRow(cfg, st))),
        h('div', { class: 'mt2' }, UI.btn('New custom style', { icon: 'plus', onClick: addStyleModal }))));

      // rules
      const rules = f.rules || [];
      const ruleList = h('div', { class: 'rule-list' });
      rules.forEach((r, i) => {
        const on = r.enabled !== false;
        const text = h('div', { class: 'rule-text', contenteditable: 'plaintext-only', spellcheck: 'false' }, r.text);
        const save = UI.debounce((v) => setRules(App.cfg().formatting.rules.map((x) => (x.id === r.id ? { ...x, text: v } : x))));
        text.addEventListener('input', () => { r.text = text.textContent; save(text.textContent); });
        text.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); text.blur(); } });
        const item = h('div', { class: `rule${on ? '' : ' off'}` },
          h('span', { class: 'rule-n' }, String(i + 1)), text,
          UI.toggle(on, (v) => { item.classList.toggle('off', !v); setRules(App.cfg().formatting.rules.map((x) => (x.id === r.id ? { ...x, enabled: v } : x))); }, 'Rule on'),
          h('button', { class: 'rule-del', type: 'button', title: 'Delete rule', on: { click: () => {
            item.classList.add('leaving');
            setTimeout(() => setRules(App.cfg().formatting.rules.filter((x) => x.id !== r.id), true), 220);
          } } }, icon('x')));
        ruleList.append(item);
      });
      const newRule = h('input', { class: 'rule-new', placeholder: rules.length ? 'Add another rule and press Enter' : 'Write a rule, e.g. “Use British spelling”, and press Enter' });
      newRule.addEventListener('keydown', (e) => {
        if (e.key !== 'Enter' || !newRule.value.trim()) return;
        setRules([...(App.cfg().formatting.rules || []), { id: 'rule-' + UI.uid().slice(0, 8), text: newRule.value.trim(), enabled: true }], true);
        setTimeout(() => { const n = document.querySelector('.rule-new'); if (n) n.focus(); }, 40);
      });
      ruleList.append(h('label', { class: 'rule add' }, h('span', { class: 'rule-n plus' }, icon('plus')), newRule));
      root.append(UI.section('Rules', 'Your own instructions, followed every time.', ruleList));

      // intensity
      const scale = h('div', { class: 'scale' },
        h('div', { class: 'scale-track' }, h('div', { class: 'scale-fill' }), h('div', { class: 'scale-knob' })),
        h('div', { class: 'scale-stops' }, ...INTENSITY.map(([v, l]) => {
          const b = h('button', { class: `scale-stop${f.intensity === v ? ' on' : ''}`, type: 'button' }, l);
          b.addEventListener('click', () => App.set('formatting.intensity', v, { render: true }));
          return b;
        })));
      scale.style.setProperty('--pos', f.intensity);
      const track = scale.querySelector('.scale-track');
      track.tabIndex = 0;
      track.setAttribute('role', 'slider');
      track.setAttribute('aria-valuemin', '0'); track.setAttribute('aria-valuemax', '3'); track.setAttribute('aria-valuenow', String(f.intensity));
      const posAt = (x) => { const r = track.getBoundingClientRect(); return Math.min(3, Math.max(0, ((x - r.left) / r.width) * 3)); };
      const commit = (v) => { const n = Math.round(v); scale.classList.remove('dragging'); scale.style.setProperty('--pos', n); if (n !== f.intensity) App.set('formatting.intensity', n, { render: true }); };
      track.addEventListener('pointerdown', (e) => {
        track.setPointerCapture(e.pointerId);
        scale.classList.add('dragging');
        scale.style.setProperty('--pos', posAt(e.clientX));
        const move = (ev) => scale.style.setProperty('--pos', posAt(ev.clientX));
        const up = (ev) => { track.removeEventListener('pointermove', move); track.removeEventListener('pointerup', up); commit(posAt(ev.clientX)); };
        track.addEventListener('pointermove', move);
        track.addEventListener('pointerup', up);
      });
      track.addEventListener('keydown', (e) => {
        const d = { ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1 }[e.key];
        if (d) { e.preventDefault(); commit(Math.min(3, Math.max(0, f.intensity + d))); }
      });
      const cur = INTENSITY[f.intensity] || INTENSITY[1];
      root.append(UI.section('How much to rewrite', null, h('div', { class: 'card pad' }, scale, h('p', { class: 'scale-desc' }, cur[2]))));

      // switches
      const opt = (ic, title, desc, ctl) => h('div', { class: 'opt' }, h('span', { class: 'opt-ic' }, icon(ic)), h('div', { class: 'grow' }, h('div', { class: 't' }, title), desc ? h('div', { class: 'd' }, desc) : ''), ctl);
      root.append(UI.section('Cleanup', 'What happens to every dictation.', h('div', { class: 'card opts' },
        opt('type', 'Auto-capitalize', 'Capitals at the start of sentences.', UI.toggle(f.autoCapitalize, (v) => App.set('formatting.autoCapitalize', v))),
        opt('textsize', 'Use what\'s in the box', 'Carries on from the text already there. Never reads passwords.', UI.toggle(App.cfg().dictation.messageContext === true || App.cfg().dictation.boxJoin === true, (v) => { App.set('dictation.messageContext', v); App.set('dictation.boxJoin', v); })))));

      let humanizer;
      root.append(UI.section('Smart formatting', 'Done on this PC, for free.', h('div', { class: 'card opts' },
        opt('note', 'Email formatting', 'Greeting, paragraphs and sign-off.', UI.toggle(f.emailFormat, (v) => App.set('formatting.emailFormat', v))),
        opt('sparkle', 'Auto-detect', 'Lists, new lines, quotes and keys like "press enter".', UI.toggle(f.autoLists, (v) => App.set('formatting.autoLists', v))),
        opt('bolt', 'Better prompts', 'Clear prompts for AI apps.', UI.toggle(f.betterPrompts !== false, (v) => App.set('formatting.betterPrompts', v, { render: true }))),
        f.betterPrompts !== false ? opt('history', 'Advanced prompting', 'Uses your recent AI chat for context.', UI.toggle(!!f.advancedPrompts, (v) => App.set('formatting.advancedPrompts', v))) : '',
        opt('check', 'Grammar polish', 'Fixes small slips.', UI.toggle(f.grammar, (v) => App.set('formatting.grammar', v))),
        humanizer = opt('wand', 'Humanizer', 'Removes stock AI phrasing.',
          UI.seg([{ value: 'off', label: 'Off' }, { value: 'light', label: 'Light' }, { value: 'strong', label: 'Strong' }], f.humanize, (v) => { humanizer.classList.toggle('lit', v !== 'off'); App.set('formatting.humanize', v); })))));
      humanizer.classList.toggle('lit', f.humanize !== 'off');

      // words to avoid
      const ex = f.excludedChars || [];
      const setEx = (list) => App.set('formatting.excludedChars', list, { render: true });
      const tagInput = h('input', { class: 'tag-input', placeholder: ex.length ? 'Add another…' : 'Type a word, phrase or character and press Enter' });
      tagInput.addEventListener('keydown', (e) => {
        const v = tagInput.value.trim();
        if ((e.key === 'Enter' || e.key === ',') && v) {
          e.preventDefault();
          if (!ex.some((x) => x.toLowerCase() === v.toLowerCase())) setEx([...ex, v]);
          setTimeout(() => { const n = document.querySelector('.tag-input'); if (n) n.focus(); }, 40);
        } else if (e.key === 'Backspace' && !tagInput.value && ex.length) setEx(ex.slice(0, -1));
      });
      const field = h('label', { class: 'tag-field' }, ...ex.map((c) => h('span', { class: 'tag' }, c,
        h('button', { type: 'button', title: 'Remove', on: { click: (e) => { e.preventDefault(); setEx(ex.filter((x) => x !== c)); } } }, icon('x')))), tagInput);
      root.append(UI.section('Words to avoid', 'Murmint never writes these.', field));

    },
  };
})();
