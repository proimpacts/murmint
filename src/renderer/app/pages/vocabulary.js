(function () {
  const { h } = UI;

  // Vocabulary fills itself: Murmint notices unusual words in what you dictate, and once one has turned up in three
  // different sentences it is suggested here. Saying "add Kubernetes to my vocabulary" while dictating also works.
  function vocabulary(cfg) {
    const p = cfg.personal;
    const box = h('div');

    const add = (t) => {
      if (p.vocabulary.some((v) => v.term.toLowerCase() === t.toLowerCase())) return;
      const seen = { ...(p.vocabSeen || {}) };
      delete seen[t.toLowerCase()];
      App.set('personal.vocabSeen', seen);
      App.set('personal.vocabulary', [...p.vocabulary, { id: UI.uid(), term: t, sounds: [], note: 'suggested' }], { render: true });
    };
    const dismiss = (w) => {
      const seen = { ...(p.vocabSeen || {}) };
      delete seen[w.term.toLowerCase()];
      App.set('personal.vocabSeen', seen);
      App.set('personal.vocabDismissed', [...(p.vocabDismissed || []), w.term.toLowerCase()], { render: true });
    };

    const suggestions = Object.values(p.vocabSeen || {}).filter((w) => (w.count || 0) >= 3).sort((a, b) => b.count - a.count);
    if (suggestions.length) {
      box.append(h('section', { class: 'svc', style: { marginBottom: '16px' } },
        h('div', { class: 'svc-head' }, h('div', null, h('h2', null, 'Suggested words', UI.badge(String(suggestions.length), 'accent')),
          h('p', null, 'Unusual words Murmint heard in several of your sentences. Add them so they are always spelled right.'))),
        h('div', { class: 'svc-body' }, ...suggestions.map((w) => h('div', { class: 'preset' },
          h('div', { class: 'grow' }, h('div', { class: 't' }, w.term, h('span', { class: 'badge' }, `heard in ${w.count} sentences`)),
            h('div', { class: 'd', title: (w.sentences || []).join(' / ') }, (w.sentences || []).slice(-1)[0] || '')),
          UI.btn('Add', { icon: 'plus', variant: 'sm primary', onClick: () => add(w.term) }),
          UI.btn('', { icon: 'x', variant: 'ghost icon', title: 'Not a word to learn', onClick: () => dismiss(w) }))))));
    }

    const list = h('div', { class: 'card flush' });
    if (!p.vocabulary.length) {
      list.append(h('div', { class: 'muted', style: { padding: '28px 22px', textAlign: 'center' } },
        'No words yet. Just dictate: names and terms Murmint hears in several sentences show up here to add.'));
    } else {
      const chips = h('div', { class: 'chips vocab-chips' });
      for (const v of p.vocabulary) {
        const label = (v.note === 'suggested' || v.note === 'auto') ? 'Suggested' : v.note === 'voice' ? 'Said aloud' : '';
        const chip = UI.chip(v.term, () => App.set('personal.vocabulary', p.vocabulary.filter((x) => x.id !== v.id), { render: true }));
        if (label) { chip.classList.add('learned'); chip.title = label; }
        chips.append(chip);
      }
      list.append(chips);
    }
    box.append(list);
    return box;
  }

  Pages.vocabulary = {
    render(root, App) {
      root.append(UI.pageHead('Vocabulary', 'Names and words Murmint should always get right. It finds them as you dictate.'));
      root.append(vocabulary(App.cfg()));
    },
  };
})();
