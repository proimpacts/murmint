'use strict';
// Fetches icons in icon-map.json from Iconify (Fluent by default, "ph:" for Phosphor).
// Run when the map changes, then build-icons.js.
const fs = require('fs');
const path = require('path');

const { icons, activeFilled } = require('./icon-map.json');

const split = (id) => (id.includes(':') ? id.split(':') : ['fluent', id]);
const filledOf = ([prefix, name]) => [prefix, prefix === 'ph' ? `${name}-fill` : name.replace('-regular', '-filled')];

(async () => {
  const wanted = new Map();
  const want = ([prefix, name]) => { if (!wanted.has(prefix)) wanted.set(prefix, new Set()); wanted.get(prefix).add(name); };
  for (const [key, id] of Object.entries(icons)) {
    want(split(id));
    if (activeFilled.includes(key)) want(filledOf(split(id)));
  }

  const found = {};
  for (const [prefix, names] of wanted) {
    const res = await fetch(`https://api.iconify.design/${prefix}.json?icons=${[...names].join(',')}`);
    const data = await res.json();
    if (data.not_found && data.not_found.length) throw new Error(`Not in ${prefix}: ${data.not_found.join(', ')}`);
    for (const name of names) {
      const i = data.icons[name];
      found[`${prefix}:${name}`] = { body: i.body, size: i.width || data.width || 24 };
    }
  }

  const out = {};
  for (const [key, id] of Object.entries(icons)) {
    const [prefix, name] = split(id);
    const regular = found[`${prefix}:${name}`];
    out[key] = { r: regular.body };
    if (regular.size !== 24) out[key].s = regular.size;
    if (activeFilled.includes(key)) out[key].f = found[filledOf([prefix, name]).join(':')].body;
  }
  const dir = path.join(__dirname, '..', 'src', 'renderer', 'assets', 'ui-icons');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'icons.json'), JSON.stringify(out, null, 1));
  console.log('icons.json written with', Object.keys(out).length, 'icons');
})().catch((e) => { console.error(e.message); process.exit(1); });
