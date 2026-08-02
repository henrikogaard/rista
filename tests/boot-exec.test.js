// Boot smoke test — executes the REAL bundled renderer in a Node VM with a
// stub DOM, so boot-time ReferenceErrors/TypeErrors (e.g. a free variable in
// initPanels, or a deleted helper still called at boot) fail this test instead
// of shipping a blank screen.
//
// The stub DOM is intentionally minimal: the app shell only needs
// getElementById / createElement / querySelector / style / classList to build
// the chrome. Any module that requires real layout (CodeMirror mounting, etc.)
// is not exercised here — that is the browser's job.

const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')
const vm = require('node:vm')

const root = path.resolve(__dirname, '..')

function makeEl(id) {
  const el = {
    id,
    children: [],
    style: { setProperty() {}, removeProperty() {} },
    dataset: {},
    className: '',
    innerHTML: '',
    textContent: '',
    _listeners: {},
    addEventListener(type, fn) { (this._listeners[type] ||= []).push(fn) },
    removeEventListener() {},
    dispatchEvent() {},
    classList: {
      add() {}, remove() {}, toggle() {}, contains() { return false },
    },
    appendChild(child) { this.children.push(child); return child },
    replaceChildren() { this.children = [] },
    setAttribute() {},
    getAttribute() { return null },
    closest() { return null },
    querySelector() { return null },
    querySelectorAll() { return [] },
    insertBefore() {},
    remove() {},
    focus() {}, blur() {}, click() {},
    getBoundingClientRect: () => ({ top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 }),
  }
  return el
}

class FakeNode {}
class FakeElement extends FakeNode {}
class FakeHTMLElement extends FakeElement {
  constructor() { super(); Object.assign(this, makeEl('html-el')) }
}

function buildSandbox() {
  const rootEl = makeEl('root')
  const els = new Map([['root', rootEl]])
  const docEl = makeEl('html')
  docEl.dataset = {}
  docEl.style = { setProperty() {}, removeProperty() {} }

  const sandbox = {
    console,
    setTimeout, clearTimeout, setInterval, clearInterval, queueMicrotask,
    Promise, Date, Math, JSON, RegExp, Array, Object, String, Number, Boolean,
    Map, Set, WeakMap, Symbol, TextEncoder, TextDecoder, URL, URLSearchParams,
    fetch: async () => ({ ok: true, json: async () => ({}) }),
    navigator: { userAgent: 'node' },
    location: { href: 'http://localhost/', search: '' },
    history: { pushState() {}, replaceState() {} },
    requestAnimationFrame: (cb) => setTimeout(cb, 0),
    cancelAnimationFrame: (id) => clearTimeout(id),
    performance: { now: () => Date.now() },
    localStorage: { getItem: () => null, setItem() {}, removeItem() {}, clear() {} },
    sessionStorage: { getItem: () => null, setItem() {}, removeItem() {}, clear() {} },
    DOMParser: class { parseFromString() { return makeEl('parsed') } },
    Node: FakeNode, Element: FakeElement, HTMLElement: FakeHTMLElement,
    getComputedStyle: () => ({ getPropertyValue: () => '', display: 'block', opacity: '1' }),
    document: {
      getElementById: (id) => els.get(id) || (els.set(id, makeEl(id)), els.get(id)),
      createElement: (tag) => makeEl(tag),
      createTextNode: (t) => ({ textContent: t }),
      createDocumentFragment: () => ({ children: [], appendChild(child) { this.children.push(child) } }),
      querySelector: () => null,
      querySelectorAll: () => [],
      documentElement: docEl,
      body: makeEl('body'),
      head: makeEl('head'),
      addEventListener() {}, removeEventListener() {},
    },
  }
  sandbox.window = sandbox
  sandbox.globalThis = sandbox
  sandbox.self = sandbox
  sandbox.addEventListener = () => {}
  sandbox.removeEventListener = () => {}
  sandbox.innerWidth = 1280
  sandbox.innerHeight = 800
  sandbox.devicePixelRatio = 1
  sandbox.matchMedia = () => ({
    matches: false, addListener() {}, removeListener() {},
    addEventListener() {}, removeEventListener() {},
  })
  return { sandbox, rootEl }
}

test('bundled renderer boots to a populated shell (no boot-time ReferenceError)', async () => {
  // Bundle the CURRENT source in-memory so the test never runs a stale dist.
  const esbuild = require('esbuild')
  const result = await esbuild.build({
    entryPoints: [path.join(root, 'src/renderer', 'index.js')],
    bundle: true,
    write: false,
    platform: 'browser',
    logLevel: 'silent',
  })
  const code = result.outputFiles[0].text

  const { sandbox, rootEl } = buildSandbox()
  let threw = null
  try {
    vm.runInNewContext(code, sandbox, { filename: 'renderer.js', timeout: 20000 })
  } catch (err) {
    threw = err
  }
  assert.equal(threw, null, `boot threw: ${threw && threw.stack}`)

  // Give async boot steps (ensureFirstRunSample, rAF) a beat, then assert the
  // shell actually rendered.
  await new Promise((resolve) => setTimeout(resolve, 100))
  assert.ok(rootEl.innerHTML.length > 1000, `#root should be populated, got ${rootEl.innerHTML.length} chars`)
  assert.ok(rootEl.innerHTML.includes('id="app"'), 'shell should contain the app root')
  assert.ok(rootEl.innerHTML.includes('id="sidebar"'), 'shell should contain the sidebar')
  assert.ok(rootEl.innerHTML.includes('Open a folder to begin'), 'shell should show the empty-state primary action')
  assert.ok(rootEl.innerHTML.includes('id="sidebar-search-btn"'), 'shell should expose the sidebar search/open control')
  assert.ok(rootEl.innerHTML.includes('statusbar'), 'shell should contain the statusbar')
})
