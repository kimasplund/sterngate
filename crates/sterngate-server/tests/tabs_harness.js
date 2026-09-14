/**
 * Tab navigation regression harness.
 *
 * Stubs a minimal DOM, evaluates the real static/js/app.js, and drives
 * switchTab() for every tab button declared in index.html. Exits non-zero if
 * any nav button fails to activate its own pane.
 *
 * Usage: node tabs_harness.js <path-to-static-dir>
 */
const fs = require('fs');
const path = require('path');

const STATIC = process.argv[2];
if (!STATIC) {
  console.error('usage: node tabs_harness.js <static-dir>');
  process.exit(2);
}

const html = fs.readFileSync(path.join(STATIC, 'index.html'), 'utf8');
const appJs = fs.readFileSync(path.join(STATIC, 'js', 'app.js'), 'utf8');

const navTabs = [...html.matchAll(/data-tab="([^"]+)"/g)].map(m => m[1]);
const paneIds = [...html.matchAll(/id="(tab-pane-[^"]+)"/g)].map(m => m[1]);

function makeEl(attrs) {
  const classes = new Set(attrs.classes || []);
  return {
    id: attrs.id || '',
    _dataTab: attrs.dataTab || null,
    classList: {
      add: c => classes.add(c),
      remove: c => classes.delete(c),
      contains: c => classes.has(c),
    },
    getAttribute: n => (n === 'data-tab' ? (attrs.dataTab ?? null) : null),
    _has: c => classes.has(c),
  };
}

const btnEls = navTabs.map(t => makeEl({ dataTab: t, classes: ['tab-btn'] }));
const paneEls = paneIds.map(id => makeEl({ id, classes: ['tab-pane'] }));

global.document = {
  querySelectorAll: sel => {
    if (sel === '.nav-tabs .tab-btn') return btnEls;
    if (sel === '.tab-pane') return paneEls;
    return [];
  },
  getElementById: () => null,
  addEventListener: () => {},
  documentElement: {},
};
global.window = { addEventListener: () => {}, dispatchEvent: () => {} };
global.localStorage = {
  _v: {},
  getItem(k) { return this._v[k] ?? null; },
  setItem(k, v) { this._v[k] = String(v); },
};
global.navigator = { language: 'en' };

const switchTab = new Function(`${appJs}\n; return switchTab;`)();

let failures = 0;
console.log(`Nav tabs declared in index.html: ${navTabs.join(', ')}`);
for (const tab of navTabs) {
  paneEls.forEach(p => p.classList.remove('active'));
  btnEls.forEach(b => b.classList.remove('active'));

  switchTab(tab);

  const pane = paneEls.find(p => p.id === `tab-pane-${tab}`);
  const btn = btnEls.find(b => b._dataTab === tab);
  const ok = pane && pane._has('active') && btn && btn._has('active');

  if (ok) {
    console.log(`  PASS  switchTab('${tab}') -> #tab-pane-${tab} active`);
  } else {
    failures++;
    const actual = paneEls.filter(p => p._has('active')).map(p => p.id).join(',') || '(none)';
    console.log(`  FAIL  switchTab('${tab}') -> expected #tab-pane-${tab}, got: ${actual}`);
  }
}

console.log(failures === 0 ? `\nAll ${navTabs.length} tabs OK` : `\n${failures} tab(s) broken`);
process.exit(failures === 0 ? 0 : 1);
