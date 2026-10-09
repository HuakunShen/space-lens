import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

const bootstrap = readFileSync(new URL('./native_material.js', import.meta.url), 'utf8');

function documentHarness(hasRoot) {
  const attributes = new Map();
  const properties = new Map();
  const events = [];
  let notify;
  let disconnected = false;
  const root = {
    dataset: {},
    setAttribute: (key, value) => attributes.set(key, value),
    toggleAttribute: (key, enabled) => enabled ? attributes.set(key, '') : attributes.delete(key),
    style: { setProperty: (key, value) => properties.set(key, value) },
  };
  const document = { documentElement: hasRoot ? root : null };
  const window = { dispatchEvent: (event) => events.push(event) };
  const context = {
    window,
    document,
    CustomEvent: class { constructor(type, init) { this.type = type; this.detail = init.detail; } },
    MutationObserver: class {
      constructor(callback) { notify = callback; }
      observe() {}
      disconnect() { disconnected = true; }
    },
  };
  runInNewContext(bootstrap, context);
  return {
    window, root, attributes, properties, events,
    insertRoot: () => { document.documentElement = root; notify(); },
    isDisconnected: () => disconnected,
    rerun: () => runInNewContext(bootstrap, context),
  };
}

test('document-start race applies the latest audited material when html arrives', () => {
  const page = documentHarness(false);
  const installed = { macos: '27', backdrop: 'glass', dark: true, accent: '#aabbcc' };
  page.window.__nativeHost.apply(installed);
  assert.equal(page.events.length, 0);
  page.insertRoot();
  assert.equal(page.attributes.get('backdrop'), 'glass');
  assert.equal(page.root.dataset.backdrop, 'glass');
  assert.equal(page.attributes.get('macos'), '27');
  assert.equal(page.attributes.has('dark'), true);
  assert.equal(page.properties.get('--color-system-accent'), '#aabbcc');
  assert.equal(page.isDisconnected(), true);
  assert.equal(page.events.at(-1).detail, installed);
});

test('initial capability is opaque; reapplying bootstrap preserves the installed host state', () => {
  const page = documentHarness(true);
  assert.equal(page.root.dataset.backdrop, 'none');
  page.window.__nativeHost.apply({ backdrop: 'vibrancy', macos: '15', dark: true, accent: '#112233' });
  page.rerun();
  assert.equal(page.root.dataset.backdrop, 'vibrancy');
  page.window.__nativeHost.apply({ backdrop: 'vibrancy', macos: '15', dark: false, accent: '#445566' });
  assert.equal(page.attributes.has('dark'), false);
  assert.equal(page.properties.get('--color-system-accent'), '#445566');
});
