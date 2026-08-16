import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
let clientModule
let source
const sources = []
const registrations = new Map()
const slotCleanups = new Map()
let controllerCleanup

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
const globalTask = {
  messageId: 'message-1',
  targetId: target.id,
  number: '1',
  status: 'in_review',
  assigneeId: actor.id,
  version: '1',
  createdAtMs: 3,
  updatedAtMs: 3,
}
const calls = []
const rpc = {
  async call(_channel, endpoint, payload) {
    calls.push({ endpoint, payload })
    const values = {
      snapshot: { actor, cursor: '7', targets: [target], followedThreadIds: [], tasks: [globalTask] },
      actors: [actor],
      'target.members': [actor],
      'history.tail': { count: '0', messages: [] },
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
  effect(factory) {
    controllerCleanup = factory()
  },
  slots: {
    inject(name, factory) {
      assert(['shell.overlay', 'sidebar.footer.action'].includes(name))
      slotCleanups.set(name, factory())
    },
    register(options, component) {
      registrations.set(options.name, { options, component })
      return () => { registrations.delete(options.name) }
    },
  },
}

clientModule.apply(ctx)
assert.equal(registrations.get('shell.overlay').options.id, 'dsh-chaos-workspace')
assert.equal(registrations.get('sidebar.footer.action').options.id, 'dsh-chaos-entry')
const injected = registrations.get('shell.overlay').options.inject()
await injected.ensure()
const state = injected.hooks.chaos.getSnapshot()
assert.equal(state.status, 'ready')
assert.equal(state.selectedTargetId, target.id)
assert.deepEqual(state.followedThreadIds, [])
assert.deepEqual(state.allTasks, [globalTask])
assert.equal(source.url, '/dsh-chaos/events?cursor=7')
assert(calls.some(call => call.endpoint === 'history.tail'))
assert(calls.every(call => call.endpoint !== 'history'))
assert(calls.some(call => call.endpoint === 'target.members'))
assert.deepEqual(state.members, [actor], 'members pane reads the membership projection, not the actor directory')
await injected.followThread('thread-1')
await injected.unfollowThread('thread-1')
assert(calls.some(call => call.endpoint === 'thread.follow'))
assert(calls.some(call => call.endpoint === 'thread.unfollow'))
injected.togglePeek()
assert.equal(injected.hooks.chaos.getSnapshot().surface, 'peek')
injected.openWorkspace()
assert.equal(injected.hooks.chaos.getSnapshot().surface, 'workspace')
injected.closeSurface()
assert.equal(injected.hooks.chaos.getSnapshot().surface, 'closed')
source.emit('change')
await new Promise(resolve => setTimeout(resolve, 0))
assert(calls.filter(call => call.endpoint === 'snapshot').length >= 2)
slotCleanups.get('shell.overlay')()
slotCleanups.get('sidebar.footer.action')()
controllerCleanup()
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
            value: { actor, cursor: String(callNumber), targets: currentTargets, followedThreadIds: [], tasks: [] },
          },
        }
      } finally {
        activeSnapshots -= 1
      }
    }
    if (endpoint === 'actors') {
      return { ok: true, value: { ok: true, value: [actor] } }
    }
    if (endpoint === 'target.members') {
      return { ok: true, value: { ok: true, value: [actor] } }
    }
    if (endpoint === 'history.tail' || endpoint === 'tasks') {
      return { ok: true, value: { ok: true, value: endpoint === 'tasks' ? [] : { count: '0', messages: [] } } }
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
        value: { ok: true, value: { actor, cursor: '99', targets: [target], followedThreadIds: [], tasks: [] } },
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

// Thread panel and send routing: opening a thread panel must not move the
// main selection, and sends must land on the right target.
const threadTarget = {
  id: 'thread-1',
  kind: 'thread',
  name: 'Thread',
  parentTargetId: target.id,
  rootMessageId: 'message-1',
  createdBy: actor.id,
  createdAtMs: 4,
}
const threadMessage = {
  seq: '9',
  id: 'message-2',
  targetId: threadTarget.id,
  authorId: actor.id,
  clientRequestId: 'req-1',
  text: 'thread reply',
  createdAtMs: 5,
}
const sendCalls = []
const threadRpc = {
  async call(_channel, endpoint, payload) {
    if (endpoint === 'snapshot') {
      return {
        ok: true,
        value: { ok: true, value: { actor, cursor: '11', targets: [target, threadTarget], followedThreadIds: [], tasks: [] } },
      }
    }
    if (endpoint === 'actors') return { ok: true, value: { ok: true, value: [actor] } }
    if (endpoint === 'target.members') return { ok: true, value: { ok: true, value: [actor] } }
    if (endpoint === 'tasks') return { ok: true, value: { ok: true, value: [] } }
    if (endpoint === 'history.tail') {
      const messages = payload.targetId === threadTarget.id ? [threadMessage] : []
      return { ok: true, value: { ok: true, value: { count: String(messages.length), messages } } }
    }
    if (endpoint === 'message.send') {
      sendCalls.push(payload)
      return { ok: true, value: { ok: true, value: { message: threadMessage, recipientIds: [], wakeAgentIds: [], replayed: false } } }
    }
    throw new Error(`unexpected endpoint ${endpoint}`)
  },
}
const threadController = new ChaosClientController(threadRpc)
await threadController.ensure()
assert.equal(threadController.getSnapshot().selectedTargetId, target.id)
await threadController.openThreadPanel(threadTarget.id)
assert.equal(threadController.getSnapshot().threadPanelId, threadTarget.id)
assert.equal(
  threadController.getSnapshot().selectedTargetId,
  target.id,
  'opening a Thread panel must not change the main selection',
)
assert.deepEqual(threadController.getSnapshot().threadPanelMessages, [threadMessage])
await threadController.send('main hello')
assert.equal(sendCalls.at(-1).targetId, target.id)
await threadController.sendToThread('thread hello')
assert.equal(sendCalls.at(-1).targetId, threadTarget.id)
threadController.closeThreadPanel()
assert.equal(threadController.getSnapshot().threadPanelId, undefined)
threadController.dispose()

// In-flight draft guard: a send that resolves late must not wipe text the
// user typed after the request started (component-level guard, unit-tested
// here through its pure decision function).
const { resolveSentDraft } = await import(`../lib/client/controller.js?smoke2=${String(Date.now())}`)
{
  // Send A; while in flight the user appends B -> A's success keeps 'AB'.
  let drafts = { 'target-1': 'A' }
  drafts = { ...drafts, 'target-1': 'AB' } // typed during the flight of A
  assert.deepEqual(
    resolveSentDraft(drafts, 'target-1', 'A'),
    { 'target-1': 'AB' },
    'late send success must preserve in-flight typing',
  )
  // Untouched draft clears normally.
  assert.deepEqual(
    resolveSentDraft({ 'target-1': 'A' }, 'target-1', 'A'),
    { 'target-1': '' },
    'untouched draft clears after send',
  )
  // Other targets' drafts are never touched.
  assert.deepEqual(
    resolveSentDraft({ 'target-1': 'A', 'thread-1': 'draft' }, 'target-1', 'A'),
    { 'target-1': '', 'thread-1': 'draft' },
    'drafts stay isolated per target',
  )
  // Compare raw snapshot, not the trimmed payload: trailing whitespace is
  // part of the sent draft and must still clear.
  assert.deepEqual(
    resolveSentDraft({ 'target-1': 'hello\n' }, 'target-1', 'hello\n'),
    { 'target-1': '' },
    'raw snapshot including trailing newline clears after send',
  )
  assert.deepEqual(
    resolveSentDraft({ 'target-1': 'hello\n' }, 'target-1', 'hello'),
    { 'target-1': 'hello\n' },
    'trimmed payload must not look like an untouched draft',
  )
}

delete globalThis.EventSource
delete globalThis.window

// CSS scope guard: the reduced-motion override must stay inside plugin
// roots — a bare universal selector would disable host transitions.
{
  const cssSource = readFileSync(new URL('../src/client/ChaosPanel.module.css', import.meta.url), 'utf8')
  const motionBlock = cssSource.match(/@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\n\}/)
  assert(motionBlock !== null, 'reduced-motion block exists')
  assert(motionBlock[0].includes('.backdrop'), 'reduced-motion is scoped under plugin roots')
  assert(!/(^|\n)\s*\*\s*[{},]/.test(motionBlock[0]), 'no bare universal selector leaks globally')
}
