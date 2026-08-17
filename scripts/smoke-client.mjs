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

// The frontend was removed wholesale; the placeholder client must stay inert.
assert.deepEqual(clientModule.inject, [])
assert.equal(typeof clientModule.apply, 'function')
assert.doesNotThrow(() => { clientModule.apply() })

console.log('smoke-client: placeholder client loads and stays inert')
