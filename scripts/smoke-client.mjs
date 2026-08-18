import assert from 'node:assert/strict'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
let clientModule

globalThis.window = {
  __ModuleLoader__: {
    load({ id, factory }) {
      assert.equal(id, 'dsh-chaos')
      clientModule = factory(specifier => require(specifier))
    },
  },
}

await import(`../lib/client.js?smoke=${String(Date.now())}`)

// The rebuilt client declares its required services and registers official
// seats only: the sidebar footer Activity entry and the shell.overlay docked
// collaboration panel. Nothing shadows or replaces shipped UI.
assert.deepEqual(clientModule.inject, ['slots', 'connection'])
assert.equal(typeof clientModule.apply, 'function')

const injected = []
const registered = []
const effects = []
const ctx = {
  get(name) {
    assert.equal(name, 'connection')
    return { rpc: { async call() { throw new Error('smoke: rpc must not fire during apply') } } }
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
assert.deepEqual(injected.sort(), ['shell.overlay', 'sidebar.footer.action'])
assert.deepEqual(
  registered.map(entry => entry.id).sort(),
  ['dsh-chaos-activity', 'dsh-chaos-dock'],
)
assert.equal(effects.length, 1)

console.log('smoke-client: activity client loads and registers official slots only')
