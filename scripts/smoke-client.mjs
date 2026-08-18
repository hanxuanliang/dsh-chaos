import assert from 'node:assert/strict'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
let clientModule

globalThis.window = {
  __ModuleLoader__: {
    load({ id, factory }) {
      assert.equal(id, 'dsh-chaos')
      clientModule = factory(specifier => {
        // The host module table serves ui-primitives in the browser; in Node
        // its real entry pulls katex CSS, so the smoke harness stubs the
        // icon-only surface the bundle actually touches.
        if (specifier === '@deepseek-ai/dsh-client-ui-primitives') {
          return new Proxy({}, { get: () => () => null })
        }
        return require(specifier)
      })
    },
  },
}

await import(`../lib/client.js?smoke=${String(Date.now())}`)

// The rebuilt client declares its required services and registers official
// seats only: the sidebar footer Activity entry, the shell.overlay docked
// collaboration panel, and the settings.section Agents page. Nothing shadows
// or replaces shipped UI.
assert.deepEqual(clientModule.inject, ['slots', 'connection'])
assert.equal(typeof clientModule.apply, 'function')

const injected = []
const registered = []
const effects = []
const ctx = {
  get(name) {
    assert.equal(name, 'connection')
    return {
      rpc: { async call() { throw new Error('smoke: rpc must not fire during apply') } },
      api: { llm: {} },
    }
  },
  effect(fn, label) {
    assert.equal(typeof fn, 'function')
    effects.push(label)
  },
  slots: {
    inject(name, factory) {
      injected.push(name)
      assert.equal(typeof factory, 'function')
      factory()
    },
    register(options, component) {
      registered.push(options)
      assert.equal(typeof component, 'function')
      return () => {}
    },
  },
}

assert.doesNotThrow(() => { clientModule.apply(ctx) })
assert.deepEqual(injected.sort(), ['settings.section', 'shell.overlay', 'sidebar.footer.action'])
assert.deepEqual(
  registered.map(entry => entry.id).sort(),
  ['dsh-chaos-activity', 'dsh-chaos-agents', 'dsh-chaos-dock'],
)
const agentsEntry = registered.find(entry => entry.id === 'dsh-chaos-agents')
assert.equal(agentsEntry.name, 'settings.section')
assert.equal(agentsEntry.order, 90)
assert.equal(agentsEntry.label(), '协作 Agents')
assert.equal(effects.length, 1)

console.log('smoke-client: activity client loads and registers official slots only')
