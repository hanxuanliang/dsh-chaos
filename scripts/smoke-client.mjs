import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
let clientModule

// The host serves packagers a fixed client module table; any require()
// outside this set bricks the whole plugin import in the browser (observed
// 2026-08-19: react-markdown/remark-* had to be force-inlined, and vfile's
// node:* shims aliased to node-min.ts). Guard the bundle here.
{
  const bundle = readFileSync(new URL('../lib/client.js', import.meta.url), 'utf8')
  const specifiers = [...bundle.matchAll(/require\("([^"]+)"\)/g)].map(m => m[1])
  const allowed = new Set([
    'react',
    'react/jsx-runtime',
    'react-dom/client',
    '@deepseek-ai/dsh-client-ui-primitives',
  ])
  assert.deepEqual([...new Set(specifiers)].sort(), [...allowed].sort())
}

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

// The P0 client declares the services it needs and registers a single
// settings.section entry (id 'chaos-agents') for the Agents management page.
assert.deepEqual(clientModule.inject, ['slots', 'connection', 'locale', 'sessions', 'conversation', 'workspaces'])
assert.equal(typeof clientModule.apply, 'function')

const injected = []
const registered = []
const effects = []
const namespaces = []
const openedPaths = []
const ctx = {
  connection: {
    rpc: { async call() { throw new Error('smoke: rpc must not fire during apply') } },
  },
  effect(fn, label) {
    assert.equal(typeof fn, 'function')
    effects.push(label)
    fn()
  },
  locale: {
    register(namespace, dictionaries) {
      namespaces.push(namespace)
      assert.ok(typeof dictionaries.zh === 'object')
      assert.ok(typeof dictionaries.en === 'object')
    },
    bind(namespace) {
      assert.equal(namespace, 'chaos')
      return key => (key === 'settings.tab' ? '协作 Agents' : key)
    },
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
  workspaces: {
    async openPath(path) { openedPaths.push(path) },
  },
}

assert.doesNotThrow(() => { clientModule.apply(ctx) })
assert.deepEqual(injected, ['settings.section'])
assert.deepEqual(registered.map(entry => entry.id), ['chaos-agents'])
const agentsEntry = registered[0]
assert.equal(agentsEntry.name, 'settings.section')
assert.equal(agentsEntry.order, 40)
assert.equal(agentsEntry.locale, 'chaos')
assert.equal(agentsEntry.label(), '协作 Agents')
const face = agentsEntry.inject()
assert.ok(face.connection === ctx.connection)
assert.equal(typeof face.openPath, 'function')
await face.openPath('/tmp/agent-workspace')
assert.deepEqual(openedPaths, ['/tmp/agent-workspace'])
assert.equal(typeof face.t, 'function')
assert.equal(face.t('settings.tab'), '协作 Agents')
assert.deepEqual(namespaces, ['chaos'])
assert.equal(effects.length, 2)
assert.ok(effects.includes('dsh-chaos: collab overlay'))

// Browser "As task" creates an unassigned todo. The model-facing task_create
// tool has its own create+claim contract and does not use CollabStore.
{
  const { CollabStore } = await import('../lib/client/data/store.js')
  let createCalls = 0
  let claimCalls = 0
  const created = {
    messageId: 'message-1',
    targetId: 'channel-1',
    number: '1',
    status: 'todo',
    version: '1',
    createdAtMs: 1,
    updatedAtMs: 1,
  }
  const store = new CollabStore({
    async taskCreate(messageId) {
      createCalls += 1
      assert.equal(messageId, created.messageId)
      return created
    },
    async taskClaim() {
      claimCalls += 1
      throw new Error('local As task must not claim')
    },
  })
  assert.deepEqual(await store.createTask(created.messageId), created)
  assert.equal(createCalls, 1)
  assert.equal(claimCalls, 0)
  assert.deepEqual(store.getSnapshot().tasksByMessage[created.messageId], created)
}

// A Thread mention picker inherits only its parent Channel's current Agents;
// an Agent present merely in the global directory must not appear.
{
  const { resolveMentionAgents } = await import('../lib/client/features/channels/mention-candidates.js')
  const user = { id: 'user-1', kind: 'user', handle: 'owner', displayName: 'Owner', createdAtMs: 1 }
  const member = { id: 'agent-1', kind: 'agent', handle: 'member', displayName: 'Member', createdAtMs: 1 }
  const outsider = { id: 'agent-2', kind: 'agent', handle: 'outsider', displayName: 'Outsider', createdAtMs: 1 }
  const actors = [user, member, outsider]
  const membersByChannel = { 'channel-1': [user, member] }
  assert.deepEqual(
    resolveMentionAgents(actors, membersByChannel, 'thread-1', 'channel-1').map(actor => actor.id),
    [member.id],
  )
  assert.deepEqual(resolveMentionAgents(actors, {}, 'thread-1', 'channel-1'), [])
  assert.deepEqual(
    resolveMentionAgents(actors, membersByChannel, 'channel-1').map(actor => actor.id),
    [member.id],
  )
}

console.log('smoke-client: client loads, registers settings, and preserves task/mention scope contracts')
