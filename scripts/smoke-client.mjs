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
// seats only: sidebar footer entry, shell.overlay workbench + agent floater +
// conversation dock, the settings.section Agents management surface, and the
// sidebar.workspaces region shadowed at priority -1.
assert.deepEqual(clientModule.inject, ['slots', 'connection', 'sessions', 'workspaces'])
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
assert.deepEqual(
  injected.sort(),
  ['settings.section', 'shell.overlay', 'shell.overlay', 'shell.overlay', 'sidebar.footer.action', 'sidebar.workspaces'],
)
assert.deepEqual(
  registered.map(entry => entry.id).sort(),
  ['dsh-chaos-agent-floater', 'dsh-chaos-agents', 'dsh-chaos-dock', 'dsh-chaos-entry', 'dsh-chaos-workbench', undefined],
)
// The sidebar region must shadow the shipped Workspace browser, never throw
// a same-priority conflict against it.
const region = registered.find(entry => entry.name === 'sidebar.workspaces')
assert.equal(region?.priority, -1)
assert.deepEqual(region?.children, undefined)
assert.equal(effects.length, 1)

console.log('smoke-client: workbench client loads and registers official slots only')
