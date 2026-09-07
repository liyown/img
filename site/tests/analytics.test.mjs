import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { JSDOM } from 'jsdom';
import ts from 'typescript';
const id = 'G-TESTONLY';
const source = await readFile(
  new URL('../src/scripts/analytics.ts', import.meta.url),
  'utf8',
);
const js = ts.transpile(source.replace('export function', 'function'), {
  target: ts.ScriptTarget.ES2022,
  module: ts.ModuleKind.ESNext,
});
function fixture(saved) {
  const dom = new JSDOM(
    '<!doctype html><title>Test</title><body><section class="consent" hidden><button data-consent="granted">Allow</button><button data-consent="denied">Decline</button><button data-consent="withdrawn">Withdraw</button></section><button data-consent-open>Preferences</button><a data-track="gui" href="#gui">GUI</a><a data-track="cli" href="#cli">CLI</a><a data-track="github" href="#github">GitHub</a><input value="private filename"></body>',
    {
      url: 'https://liyown.github.io/img/?secret=private',
      runScripts: 'outside-only',
    },
  );
  const { window: w } = dom;
  if (saved) w.localStorage.setItem('img.analytics-consent.v1', saved);
  w.eval(js + `\ninitAnalytics('${id}');`);
  const click = (selector) => w.document.querySelector(selector).click();
  const scripts = () =>
    w.document.querySelectorAll('script[src*="googletagmanager"]').length;
  const calls = () => Array.from(w.dataLayer || []).map((x) => Array.from(x));
  return { w, click, scripts, calls, close: () => w.close() };
}
test('fresh or denied visitor sends no request and stores a refusal', () => {
  for (const saved of [undefined, 'denied', 'withdrawn']) {
    const f = fixture(saved);
    assert.equal(f.scripts(), 0);
    assert.equal(f.w.dataLayer, undefined);
    assert.equal(f.w[`ga-disable-${id}`], true);
    f.click('[data-consent="denied"]');
    assert.equal(f.scripts(), 0);
    assert.equal(
      f.w.localStorage.getItem('img.analytics-consent.v1'),
      'denied',
    );
    f.close();
  }
});
test('consent loads once and sends one sanitized pageview plus allowlisted entries', () => {
  const f = fixture();
  f.click('[data-consent="granted"]');
  f.click('[data-consent="granted"]');
  assert.equal(f.scripts(), 1);
  f.click('[data-track="gui"]');
  f.click('[data-track="cli"]');
  f.click('[data-track="github"]');
  assert.equal(
    f.calls().filter((c) => c[0] === 'event' && c[1] === 'page_view').length,
    1,
  );
  assert.equal(f.calls().filter((c) => c[1] === 'entry_click').length, 3);
  const payload = JSON.stringify(f.calls());
  assert.ok(!payload.includes('?secret'));
  assert.ok(!payload.includes('private filename'));
  assert.equal(
    f.calls().find((c) => c[0] === 'config')[2].send_page_view,
    false,
  );
  f.close();
});
test('withdrawal disables subsequent tracking, clears cookies and supports a later grant', () => {
  const f = fixture('granted');
  f.w.document.cookie = '_ga=fixture; path=/img/';
  f.w.document.cookie = '_ga_TESTONLY=fixture; path=/';
  f.click('[data-consent="withdrawn"]');
  assert.equal(f.w[`ga-disable-${id}`], true);
  assert.equal(f.scripts(), 0);
  assert.equal(f.w.document.cookie, '');
  f.click('[data-track="gui"]');
  assert.equal(f.calls().length, 0);
  f.click('[data-consent-open]');
  assert.equal(f.w.document.querySelector('.consent').hidden, false);
  f.click('[data-consent="granted"]');
  assert.equal(f.scripts(), 1);
  assert.equal(f.w[`ga-disable-${id}`], false);
  assert.equal(f.calls().filter((c) => c[1] === 'page_view').length, 0);
  f.close();
});
test('saved grant loads on a new page; cross-tab withdrawal stops tracking', () => {
  const f = fixture('granted');
  assert.equal(f.scripts(), 1);
  assert.equal(f.calls().filter((c) => c[1] === 'page_view').length, 1);
  f.w.dispatchEvent(
    new f.w.StorageEvent('storage', {
      key: 'img.analytics-consent.v1',
      newValue: 'withdrawn',
    }),
  );
  f.click('[data-track="cli"]');
  assert.equal(f.calls().length, 0);
  assert.equal(f.w[`ga-disable-${id}`], true);
  f.close();
});
test('missing production ID means no consent UI, GA ID, or Google script', async () => {
  const html = await readFile(
    new URL('../dist/index.html', import.meta.url),
    'utf8',
  );
  const doc = new JSDOM(html).window.document;
  assert.equal(doc.querySelector('[data-ga-id]'), null);
  assert.equal(doc.querySelector('.consent'), null);
  assert.equal(doc.querySelector('script[src*="google"]'), null);
});
