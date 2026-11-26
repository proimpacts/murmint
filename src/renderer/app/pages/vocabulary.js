(function () {
  const { h, icon } = UI;
  const api = window.murmint;
  let tab = 'terms';

  const chipsInput = (values, onChange, placeholder, canAdd = true) => {
    const wrap = h('div', { class: 'chips' });
    const draw = () => {
      wrap.replaceChildren(...values.map((v, i) => UI.chip(v, () => { values.splice(i, 1); onChange(values); draw(); })));
      if (!canAdd) return;
      const inp = UI.input({ placeholder, cls: 'w-sm' });
      inp.style.height = '26px';
      inp.addEventListener('keydown', (e) => {
        if ((e.key === 'Enter' || e.key === ',') && inp.value.trim()) { e.preventDefault(); values.push(inp.value.trim()); onChange(values); draw(); wrap.querySelector('input').focus(); }
      });
      wrap.append(inp);
    };
    draw();
    return wrap;
  };

  const lettersOf = (x) => String(x).toLowerCase().replace(/[^a-z0-9]/g, '');
  const mergeSounds = (existing, variants, term) => {
    const seen = new Set([lettersOf(term), ...(existing || []).map(lettersOf)]);
    const out = [...(existing || [])];
    for (const v of variants) { const k = lettersOf(v); if (k && !seen.has(k)) { seen.add(k); out.push(v); } }
    return out;
  };

  const LEVELS = [
    { id: 'normal', label: 'Normal', say: 'Say it the way you normally would.' },
    { id: 'quiet', label: 'Quiet', say: 'Now say it quietly, like you do not want to be overheard.' },
    { id: 'fast', label: 'Fast', say: 'Now say it fast, like it is in the middle of a sentence.' },
    { id: 'slow', label: 'Slow and clear', say: 'Now say it slowly and clearly, one sound at a time.' },
    { id: 'loud', label: 'Loud', say: 'Now say it loudly, as if across a room.' },
  ];

  function trainModal(target) {
    const entryId = typeof target === 'string' ? target : null;
    const first = entryId ? App.cfg().personal.vocabulary.find((v) => v.id === entryId) : null;
    const term = entryId ? (first && first.term) : target && target.term;
    if (!term) return;
    const NEED = LEVELS.length;
    const takes = [];
    let rec = null, finished = false;
    const status = h('div', { class: 'muted', style: { minHeight: '40px' } }, 'Starting the microphone...');
    const meter = h('div', { class: 'meter' }, h('i'));
    const rows = h('div', { class: 'card flush mt2' });
    const footer = h('div', { class: 'flex mt3', style: { justifyContent: 'flex-end' } });
    const root = h('div', { id: 'vocab-trainer' },
      h('h3', null, 'Teach Murmint how you say it'),
      h('div', { style: { fontSize: '26px', fontWeight: 650, letterSpacing: '-.03em', margin: '10px 0 4px' } }, term),
      h('p', { class: 'muted', style: { margin: '0 0 14px' } }, 'You will say it five times, a little differently each time: normal, quiet, fast, slow and loud. People do not say a word the same way twice, so Murmint learns all of those versions.'),
      meter, h('div', { class: 'mt2' }, status), rows, footer);

    const drawRows = () => rows.replaceChildren(...Array.from({ length: NEED }, (_, i) => {
      const t = takes[i];
      const text = !t ? (i === takes.length ? LEVELS[i].say : '') : t.state === 'listening' ? UI.loader('Listening') : t.state === 'error' ? t.error
        : t.empty ? 'Could not make out a word' : t.exact ? 'Heard it correctly' : '\u201c' + (t.variant || t.heard) + '\u201d';
      return h('div', { class: 'item flex between', style: { padding: '12px 18px' } }, h('span', { class: 'muted' }, LEVELS[i].label),
        h('span', { style: { color: t && t.state === 'error' ? 'var(--bad)' : '' } }, text, t && t.state === 'done' && t.exact ? UI.badge('Good', 'good') : null));
    }));
    const drawFooter = (done) => footer.replaceChildren(...[
      done ? UI.btn('Record again', { variant: 'ghost', onClick: () => { close(); setTimeout(() => trainModal(entryId || { term }), 50); } }) : null,
      UI.btn(done ? 'Done' : takes.some((t) => t && t.state === 'done') ? 'Finish now' : 'Cancel', { variant: 'primary', onClick: () => { if (!done && takes.some((t) => t && t.state === 'done')) finish(); else close(); } }),
    ].filter(Boolean));
    const stopRec = () => { try { if (rec) { rec.cancel(); rec._teardown(); } } catch { } rec = null; };

    const finish = () => {
      if (finished) return;
      finished = true; stopRec();
      const done = takes.map((t, i) => (t && t.state === 'done' ? { ...t, level: LEVELS[i].id } : null)).filter(Boolean);
      const variants = done.filter((t) => t.variant).map((t) => t.variant);
      const exact = done.filter((t) => t.exact).length;
      const list = App.cfg().personal.vocabulary;
      const levels = done.filter((t) => t.variant || t.exact).map((t) => ({ level: t.level, heard: t.variant || term }));
      if (variants.length || exact) {
        if (entryId) App.set('personal.vocabulary', list.map((v) => (v.id === entryId ? { ...v, sounds: mergeSounds(v.sounds, variants, v.term), levels, note: 'trained', trained: true } : v)));
        else if (!list.some((v) => v.term.toLowerCase() === term.toLowerCase())) App.set('personal.vocabulary', [...list, { id: UI.uid(), term, sounds: mergeSounds([], variants, term), levels, note: 'trained', trained: true }]);
        App.render();
      }
      meter.firstChild.style.width = '0';
      status.textContent = variants.length
        ? 'Learned. Whenever Murmint hears ' + [...new Set(variants)].map((x) => '\u201c' + x + '\u201d').join(' or ') + ' (or anything that sounds like it) it will write \u201c' + term + '\u201d.'
        : exact ? 'Murmint already hears this one correctly, so nothing extra was needed. It is saved with its spelling.'
          : 'No clear take was heard, so the word was not added. A word is only added once Murmint has heard you say it. Try again a little closer to the microphone.';
      drawFooter(true);
    };

    const onSegment = async (seg) => {
      if (finished || seg.final || takes.length >= NEED) return;
      const i = takes.length;
      takes[i] = { state: 'listening' }; drawRows();
      status.textContent = 'Got it. Listening to that...';
      const r = await window.murmint.trainVocab({ wav: seg.wav, durationSec: seg.durationSec, term });
      if (finished) return;
      takes[i] = r.ok ? { state: 'done', ...r } : { state: 'error', error: r.error };
      drawRows();
      if (!r.ok) { finished = true; stopRec(); status.textContent = r.error; drawFooter(true); return; }
      if (takes.length >= NEED && takes.every((t) => t.state !== 'listening')) return finish();
      status.textContent = LEVELS[takes.length].say;
      drawFooter(false);
    };

    const close = UI.modal(root);
    drawRows(); drawFooter(false);
    const watcher = setInterval(() => { if (!document.getElementById('vocab-trainer')) { clearInterval(watcher); finished = true; stopRec(); } }, 400);

    rec = new Recorder({ onLevel: (v) => { meter.firstChild.style.width = Math.min(100, v * 900) + '%'; }, onSegment, onPreview() {}, onCaption() {}, onChunk() {}, onBoundary() {}, onError: (m) => { status.textContent = m; } });
    const cfg = App.cfg();
    setTimeout(async () => {
      if (finished) return;
      const ok = await rec.start({ deviceId: cfg.dictation.micDeviceId, continuous: true, stream: false, preview: false, silenceMs: 650, noiseGate: cfg.dictation.noiseGate, workletUrl: '../overlay/recorder-worklet.js' });
      if (ok && !finished) status.textContent = LEVELS[0].say;
    }, 250);
  }

  function termsTab(cfg) {
    const p = cfg.personal;
    const box = h('div');
    const term = UI.input({ placeholder: 'A word or name, e.g. Kubernetes, Siobhan, yapping', cls: 'w-lg' });
    const add = (t, from) => {
      t = (t || '').trim();
      if (!t) return UI.toast('Type the word first', 'error');
      if (p.vocabulary.some((v) => v.term.toLowerCase() === t.toLowerCase())) return UI.toast(`${t} is already in your vocabulary`, 'info');
      const seen = { ...(p.vocabSeen || {}) };
      delete seen[t.toLowerCase()];
      App.set('personal.vocabSeen', seen);
      App.set('personal.vocabulary', [...p.vocabulary, { id: UI.uid(), term: t, sounds: [], note: from }], { render: true });
    };
    term.addEventListener('keydown', (e) => { if (e.key === 'Enter') { add(term.value, 'manual'); term.value = ''; } });

    // words Murmint noticed in several of your sentences; nothing is added without a click
    const suggestions = Object.values(p.vocabSeen || {}).filter((w) => (w.count || 0) >= 3).sort((a, b) => b.count - a.count);
    if (suggestions.length) {
      const dismiss = (w) => {
        const seen = { ...(p.vocabSeen || {}) };
        delete seen[w.term.toLowerCase()];
        App.set('personal.vocabSeen', seen);
        App.set('personal.vocabDismissed', [...(p.vocabDismissed || []), w.term.toLowerCase()], { render: true });
      };
      box.append(h('section', { class: 'svc', style: { marginBottom: '16px' } },
        h('div', { class: 'svc-head' }, h('div', null, h('h2', null, 'Suggested words', UI.badge(String(suggestions.length), 'accent')),
          h('p', null, 'Unusual words Murmint heard in several of your sentences. Add them so they are always spelled right.'))),
        h('div', { class: 'svc-body' }, ...suggestions.map((w) => h('div', { class: 'preset' },
          h('div', { class: 'grow' }, h('div', { class: 't' }, w.term, h('span', { class: 'badge' }, `heard in ${w.count} sentences`)),
            h('div', { class: 'd', title: (w.sentences || []).join(' / ') }, (w.sentences || []).slice(-1)[0] || '')),
          UI.btn('Add', { icon: 'plus', variant: 'sm primary', onClick: () => add(w.term, 'auto') }),
          UI.btn('', { icon: 'x', variant: 'ghost icon', title: 'Not a word to learn', onClick: () => dismiss(w) }))))));
    }

    box.append(UI.card(
      h('div', { class: 'row stack' },
        h('div', null, h('div', { class: 't' }, 'Add a word'), h('div', { class: 'd' }, 'Type a name or term and press Add. You can also say "add Kubernetes to my vocabulary" while dictating.')),
        h('div', { class: 'flex wrap' }, term, UI.btn('Add', { icon: 'plus', variant: 'primary', onClick: () => { add(term.value, 'manual'); term.value = ''; } })))));

    const list = h('div', { class: 'card flush mt2' });
    if (!p.vocabulary.length) list.append(h('div', { class: 'muted', style: { padding: '28px 22px', textAlign: 'center' } }, 'No words yet. Add one above, or add the words Murmint suggests as you dictate.'));
    for (const v of p.vocabulary) {
      const heard = v.sounds || [];
      const badge = v.trained ? UI.badge('Recorded', 'good') : v.note === 'voice' ? UI.badge('Said aloud', 'accent') : v.note === 'auto' ? UI.badge('Suggested', 'accent') : null;
      list.append(h('div', { class: 'item' },
        h('div', { class: 'flex between' },
          h('div', { class: 'flex' }, h('b', { style: { fontSize: '15px' } }, v.term), badge),
          h('div', { class: 'flex', style: { gap: '4px' } },
            UI.btn(v.trained ? 'Re-record' : 'Record', { icon: 'mic', variant: 'sm', onClick: () => trainModal(v.id) }),
            UI.btn('', { icon: 'trash', variant: 'ghost icon', title: 'Remove', onClick: () => App.set('personal.vocabulary', p.vocabulary.filter((x) => x.id !== v.id), { render: true }) }))),
        ));
    }
    box.append(list);
    return box;
  }

  Pages.vocabulary = {
    render(root, App) {
      const cfg = App.cfg();
      root.append(UI.pageHead('Vocabulary', 'Names and words Murmint should always get right.'));
      root.append(h('div', { class: 'mt3' }, termsTab(cfg)));
    },
  };
})();
