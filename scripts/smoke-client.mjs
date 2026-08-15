import assert from 'node:assert/strict'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
let clientModule
let registration
let cleanup
let source
let slotFactory
const sources = []

class FakeEventSource {
  listeners = new Map()
  closed = false

  constructor(url) {
    this.url = url
    source = this
    sources.push(this)
    queueMicrotask(() => { this.onopen?.() })
  }

  addEventListener(name, listener) {
    this.listeners.set(name, listener)
  }

  emit(name) {
    this.listeners.get(name)?.()
  }

  close() {
    this.closed = true
  }
}

function deferred() {
  let resolve
  const promise = new Promise(done => { resolve = done })
  return { promise, resolve }
}

async function until(predicate, description) {
  const deadline = Date.now() + 2_000
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${description}`)
    await new Promise(resolve => setTimeout(resolve, 0))
  }
}

globalThis.EventSource = FakeEventSource
globalThis.window = {
  __ModuleLoader__: {
    load({ id, factory }) {
      assert.equal(id, 'dsh-chaos')
      clientModule = factory(specifier => require(specifier))
    },
  },
}

await import(`../lib/client.js?smoke=${String(Date.now())}`)
assert.deepEqual(clientModule.inject, ['slots', 'connection'])

const actor = {
  id: 'browser-1',
  kind: 'user',
  handle: 'browser',
  displayName: 'Browser',
  createdAtMs: 1,
}
const target = {
  id: 'target-1',
  kind: 'channel',
  name: 'Design',
  createdBy: actor.id,
  createdAtMs: 2,
}
const calls = []
const rpc = {
  async call(_channel, endpoint, payload) {
    calls.push({ endpoint, payload })
    const values = {
      snapshot: { actor, cursor: '7', targets: [target], tasks: [] },
      actors: [actor],
      history: [],
      tasks: [],
    }
    return { ok: true, value: { ok: true, value: values[endpoint] ?? null } }
  },
}
const ctx = {
  get(name) {
    assert.equal(name, 'connection')
    return { rpc }
  },
  slots: {
    inject(name, factory) {
      assert.equal(name, 'shell.overlay')
      slotFactory = factory
      cleanup = factory()
    },
    register(options, component) {
      registration = { options, component }
      return () => { registration = undefined }
    },
  },
}

clientModule.apply(ctx)
assert.equal(registration.options.id, 'dsh-chaos')
const injected = registration.options.inject()
await injected.ensure()
const state = injected.hooks.chaos.getSnapshot()
assert.equal(state.status, 'ready')
assert.equal(state.selectedTargetId, target.id)
assert.equal(source.url, '/dsh-chaos/events?cursor=7')
assert(calls.some(call => call.endpoint === 'history'))
source.emit('change')
await new Promise(resolve => setTimeout(resolve, 0))
assert(calls.filter(call => call.endpoint === 'snapshot').length >= 2)
cleanup()
assert.equal(source.closed, true)
const remountSourceCount = sources.length
cleanup = slotFactory()
const remountInjected = registration.options.inject()
await remountInjected.ensure()
assert.equal(sources.length, remountSourceCount + 1, 'slot remount needs a fresh controller')
cleanup()
assert.equal(source.closed, true)

const { ChaosClientController } = await import(`../lib/client/controller.js?smoke=${String(Date.now())}`)
const secondTarget = {
  id: 'target-2',
  kind: 'channel',
  name: 'Implementation',
  createdBy: actor.id,
  createdAtMs: 3,
}
let currentTargets = [target]
let snapshotCalls = 0
let activeSnapshots = 0
let maximumActiveSnapshots = 0
let snapshotGate
const directRpc = {
  async call(_channel, endpoint) {
    if (endpoint === 'snapshot') {
      snapshotCalls += 1
      const callNumber = snapshotCalls
      activeSnapshots += 1
      maximumActiveSnapshots = Math.max(maximumActiveSnapshots, activeSnapshots)
      try {
        if (snapshotGate !== undefined) await snapshotGate.promise
        return {
          ok: true,
          value: {
            ok: true,
            value: { actor, cursor: String(callNumber), targets: currentTargets, tasks: [] },
          },
        }
      } finally {
        activeSnapshots -= 1
      }
    }
    if (endpoint === 'actors') {
      return { ok: true, value: { ok: true, value: [actor] } }
    }
    if (endpoint === 'history' || endpoint === 'tasks') {
      return { ok: true, value: { ok: true, value: [] } }
    }
    if (endpoint === 'channel.create') {
      currentTargets = [...currentTargets, secondTarget]
      return { ok: true, value: { ok: true, value: secondTarget } }
    }
    throw new Error(`unexpected endpoint ${endpoint}`)
  },
}
const controller = new ChaosClientController(directRpc)
await controller.ensure()
const directSource = source

snapshotGate = deferred()
directSource.emit('change')
await until(() => activeSnapshots === 1, 'first refresh to start')
directSource.emit('change')
await new Promise(resolve => setTimeout(resolve, 0))
assert.equal(maximumActiveSnapshots, 1, 'projection reloads must be serialized')
const gate = snapshotGate
snapshotGate = undefined
gate.resolve()
await until(() => snapshotCalls >= 3 && activeSnapshots === 0, 'coalesced refresh to drain')

const beforeResyncSources = sources.length
directSource.emit('resync_required')
await until(() => sources.length === beforeResyncSources + 1, 'full snapshot resync')
const recoveredSource = source
assert.equal(directSource.closed, true)
assert.equal(
  recoveredSource.url,
  `/dsh-chaos/events?cursor=${controller.getSnapshot().cursor}`,
)

await controller.createChannel('Implementation')
assert.equal(controller.getSnapshot().selectedTargetId, secondTarget.id)
currentTargets = []
await controller.refresh()
assert.equal(controller.getSnapshot().selectedTargetId, undefined)
assert.deepEqual(controller.getSnapshot().messages, [])
assert.deepEqual(controller.getSnapshot().tasks, [])
controller.dispose()
assert.equal(recoveredSource.closed, true)

const pendingGate = deferred()
let pendingSnapshotStarted = false
const pendingRpc = {
  async call(_channel, endpoint) {
    if (endpoint === 'snapshot') {
      pendingSnapshotStarted = true
      await pendingGate.promise
      return {
        ok: true,
        value: { ok: true, value: { actor, cursor: '99', targets: [target], tasks: [] } },
      }
    }
    if (endpoint === 'actors') return { ok: true, value: { ok: true, value: [actor] } }
    throw new Error(`unexpected endpoint ${endpoint}`)
  },
}
const pendingController = new ChaosClientController(pendingRpc)
const sourceCount = sources.length
const pendingEnsure = pendingController.ensure()
await until(() => pendingSnapshotStarted, 'pending initial snapshot')
pendingController.dispose()
pendingGate.resolve()
await pendingEnsure
assert.equal(sources.length, sourceCount, 'dispose must prevent a late EventSource')

delete globalThis.EventSource
delete globalThis.window
