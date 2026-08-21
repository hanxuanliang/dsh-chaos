import assert from 'node:assert/strict'
import { DeliveryBridge, RuntimeManager } from '../lib/index.js'

class FakeCollab {
  bindings = new Map()
  pending = []
  notified = []
  seen = []
  sends = []
  rearms = []
  taskActors = []
  published = false

  async bindRuntime(agentId, sessionId, provider, model, preset) {
    assert.equal(this.published, true, 'binding must follow DSH publication')
    const previous = this.bindings.get(agentId)
    const binding = {
      agentId,
      sessionId,
      generation: String(Number(previous?.generation ?? 0) + 1),
      provider,
      model,
      preset,
      boundAtMs: Date.now(),
    }
    this.bindings.set(agentId, binding)
    return binding
  }

  async runtimeBinding(agentId) { return this.bindings.get(agentId) }
  async updateRuntimePreset(agentId, generation, sessionId, preset) {
    const binding = this.bindings.get(agentId)
    assert.equal(binding.generation, generation)
    assert.equal(binding.sessionId, sessionId)
    const updated = { ...binding, preset }
    this.bindings.set(agentId, updated)
    return updated
  }
  async runtimeBindingForSession(sessionId) {
    return [...this.bindings.values()].find(binding => binding.sessionId === sessionId)
  }
  async listRuntimeBindings() { return [...this.bindings.values()] }
  async listPendingWakes() { return this.pending }
  async markNotified(agentId, generation, sessionId, pendingSeq) {
    this.notified.push({ agentId, generation, sessionId, pendingSeq })
    this.pending = this.pending.filter(wake => wake.binding.agentId !== agentId)
  }
  async rearmRuntimeWake(agentId, generation, sessionId) {
    this.rearms.push({ agentId, generation, sessionId })
  }
  async checkInbox(agentId, generation, sessionId, limit) {
    assert.equal(limit, 7)
    return {
      id: 'batch-1',
      agentId,
      sessionId,
      generation,
      messages: [{
        deliveryId: 'delivery-1',
        message: {
          seq: '9',
          id: 'message-1',
          targetId: 'target-1',
          authorId: 'human-1',
          clientRequestId: 'human-send-1',
          text: 'private body',
          createdAtMs: 1,
        },
      }],
      contexts: [await this.identityContext(agentId, 'target-1')],
      checkedAtMs: 2,
    }
  }
  async identityContext(agentId, targetId) {
    return {
      agent: {
        actor: {
          id: agentId,
          kind: 'agent',
          handle: 'alpha',
          displayName: 'Alpha',
          createdAtMs: 1,
        },
        workspacePath: '/tmp/dsh-chaos-agent-1',
        lifecycle: 'active',
        charter: {
          schemaVersion: 1,
          summary: 'Own implementation',
          capabilities: ['rust'],
          constraints: [],
        },
        version: '1',
        createdAtMs: 1,
        updatedAtMs: 1,
      },
      ...(targetId === undefined ? {} : {
        target: {
          id: targetId,
          kind: 'channel',
          name: 'design',
          createdBy: 'human-1',
          createdAtMs: 1,
        },
        membershipTarget: {
          id: targetId,
          kind: 'channel',
          name: 'design',
          createdBy: 'human-1',
          createdAtMs: 1,
        },
      }),
      members: [{
        actor: {
          id: agentId,
          kind: 'agent',
          handle: 'alpha',
          displayName: 'Alpha',
          createdAtMs: 1,
        },
        role: 'member',
        joinedAtMs: 1,
      }],
    }
  }
  async markModelSeen(batchId, agentId, generation, sessionId) {
    this.seen.push({ batchId, agentId, generation, sessionId })
  }
  async sendMessage(input) {
    this.sends.push(input)
    return {
      message: {
        seq: '10',
        id: 'message-2',
        targetId: input.targetId,
        authorId: input.authorId,
        clientRequestId: input.clientRequestId,
        text: input.text,
        createdAtMs: 3,
      },
      recipientIds: [],
      wakeAgentIds: [],
      replayed: false,
    }
  }
  async readMessage(actorId, targetId, messageId) {
    assert.equal(actorId, 'agent-1')
    return {
      seq: '10',
      id: messageId,
      targetId,
      authorId: 'agent-1',
      clientRequestId: 'call-1',
      text: 'reply',
      createdAtMs: 3,
    }
  }
  async readMessages(actorId, targetId, afterSeq, limit) {
    assert.equal(actorId, 'agent-1')
    assert.equal(afterSeq, '0')
    assert.equal(limit, 5)
    return [await this.readMessage(actorId, targetId, 'message-2')]
  }
  async createTask(messageId, actorId) {
    this.taskActors.push(actorId)
    return {
      messageId,
      targetId: 'target-1',
      number: '1',
      status: 'todo',
      version: '1',
      createdAtMs: 4,
      updatedAtMs: 4,
    }
  }
  async claimTask(messageId, actorId) {
    this.taskActors.push(actorId)
    return {
      messageId,
      targetId: 'target-1',
      number: '1',
      status: 'in_progress',
      assigneeId: actorId,
      version: '2',
      createdAtMs: 4,
      updatedAtMs: 5,
    }
  }
  async listTasks(actorId, targetId) {
    this.taskActors.push(actorId)
    assert.equal(targetId, 'target-1')
    return [await this.claimTask('message-2', actorId)]
  }
  async updateTaskStatus(messageId, actorId, status, expectedVersion) {
    this.taskActors.push(actorId)
    assert.equal(expectedVersion, '2')
    return {
      messageId,
      targetId: 'target-1',
      number: '1',
      status,
      assigneeId: actorId,
      version: '3',
      createdAtMs: 4,
      updatedAtMs: 6,
    }
  }
  async unclaimTask(messageId, actorId, expectedVersion) {
    this.taskActors.push(actorId)
    assert.equal(expectedVersion, '3')
    return {
      messageId,
      targetId: 'target-1',
      number: '1',
      status: 'in_review',
      version: '4',
      createdAtMs: 4,
      updatedAtMs: 7,
    }
  }
}

const tools = new Map()
const notices = []
const steers = []
const sessionEvents = []
let disposed = 0
let createdOptions
let failNextCreate = false
const collab = new FakeCollab()
const fakeAgent = {
  id: 'session-1',
  status: 'idle',
  followup: message => notices.push(message),
  steer: message => steers.push(message),
  session: { append: (type, data) => { sessionEvents.push({ type, data }) } },
}
const registry = {
  async create(options) {
    createdOptions = options
    if (failNextCreate) {
      failNextCreate = false
      throw new Error('injected create failure')
    }
    await options.setup({
      on: () => {},
      tools: { register: definition => { tools.set(definition.name, definition) } },
    })
    collab.published = true
    fakeAgent.id = String(options.sessionId)
    return { agent: fakeAgent, dispose: async () => { disposed += 1 } }
  },
  async resume(options) {
    await options.setup({
      on: () => {},
      tools: { register: definition => { tools.set(definition.name, definition) } },
    })
    fakeAgent.id = String(options.resumeSessionId)
    return { agent: fakeAgent, dispose: async () => { disposed += 1 } }
  },
}
const mountedPresets = []
const presets = {
  async resolve(id) {
    if (id === 'default') throw new Error('unknown legacy sentinel')
    return { id: id ?? 'standard' }
  },
  async mount(_ctx, id) {
    mountedPresets.push(id)
    return { id }
  },
}
const warningErrors = []
const warnings = { warn: (message, error) => warningErrors.push({ message, error }) }
const runtimes = new RuntimeManager(registry, presets, collab, warnings)
const binding = await runtimes.create({
  agentId: 'agent-1',
  workspacePath: '/tmp/dsh-chaos-agent-1',
  provider: 'openai',
  model: 'codex',
  preset: 'default',
  sessionId: 'session-1',
})
assert.equal(createdOptions.meta.agentPreset, 'standard')
assert.equal(binding.preset, 'standard')
assert.deepEqual(mountedPresets, ['standard'])
assert.equal(runtimes.resolve(binding), fakeAgent)
assert.deepEqual([...tools.keys()].sort(), [
  'identity_context',
  'message_check',
  'message_read',
  'message_send',
  'task_claim',
  'task_create',
  'task_list',
  'task_unclaim',
  'task_update',
])

collab.pending = [{ binding, pendingSeq: '9' }]
const bridge = new DeliveryBridge(collab, runtimes, warnings, 50)
await bridge.scanOnce()
assert.equal(notices.length, 1)
assert.equal(steers.length, 0)
assert.equal(notices[0].content[0].text.includes('private body'), false)
assert.equal(collab.notified.length, 1)

const execution = {
  callId: 'call-1',
  signal: new AbortController().signal,
  agent: fakeAgent,
}
const checked = await tools.get('message_check').execute({ limit: 7 }, execution)
assert.equal(checked.messages[0].message.text, 'private body')
assert.equal(checked.contexts[0].members[0].actor.handle, 'alpha')
const identity = await tools.get('identity_context').execute({ targetId: 'target-1' }, execution)
assert.equal(identity.target.name, 'design')
assert.equal(identity.agent.actor.handle, 'alpha')
assert.equal(collab.seen.length, 0, 'check alone must not mark model-seen')
await runtimes.rearmUnconfirmed(fakeAgent)
assert.equal(collab.rearms.length, 1, 'idle checked work must be re-armed')
await runtimes.confirmModelSeen('session-1')
assert.equal(collab.seen.length, 1)

const sent = await tools.get('message_send').execute({
  targetId: 'target-1',
  text: 'reply',
  authorId: 'forged-agent',
}, execution)
assert.equal(sent.message.authorId, 'agent-1')
assert.equal(collab.sends[0].clientRequestId, 'call-1')

const history = await tools.get('message_read').execute({
  targetId: 'target-1',
  afterSeq: '0',
  limit: 5,
  actorId: 'forged-agent',
}, execution)
assert.equal(history.messages[0].id, 'message-2')
const createdTask = await tools.get('task_create').execute({
  messageId: 'message-2',
  actorId: 'forged-agent',
}, execution)
// 方案A(2026-08-19): task_create = create+claim — in_progress
assert.equal(createdTask.status, 'in_progress')
const claimedTask = await tools.get('task_claim').execute({
  messageId: 'message-2',
  actorId: 'forged-agent',
}, execution)
assert.equal(claimedTask.assigneeId, 'agent-1')
const listedTasks = await tools.get('task_list').execute({ targetId: 'target-1' }, execution)
assert.equal(listedTasks.tasks[0].assigneeId, 'agent-1')
const reviewedTask = await tools.get('task_update').execute({
  messageId: 'message-2',
  status: 'in_review',
  expectedVersion: '2',
}, execution)
assert.equal(reviewedTask.version, '3')
const unclaimedTask = await tools.get('task_unclaim').execute({
  messageId: 'message-2',
  expectedVersion: '3',
}, execution)
assert.equal(unclaimedTask.assigneeId, undefined)
// 方案A后 create+claim 于工具内连调 → 由业 mock 含 (1 create + 2 claim + 1 list
// + 1 update + 1 unclaim + 1 claim-again 不): 实际 7 动; 老断言 6 = 前方案A。
assert.deepEqual(collab.taskActors, [
  'agent-1',
  'agent-1',
  'agent-1',
  'agent-1',
  'agent-1',
  'agent-1',
  'agent-1',
])

fakeAgent.status = 'running'
collab.pending = [{ binding, pendingSeq: '10' }]
await bridge.scanOnce()
assert.equal(steers.length, 1)

const reset = await runtimes.reset({
  agentId: 'agent-1',
  workspacePath: '/tmp/dsh-chaos-agent-1',
  provider: 'openai',
  model: 'codex',
  preset: 'default',
  sessionId: 'session-2',
}, binding.generation)
assert.equal(reset.generation, '2')
assert.equal(runtimes.resolve(binding), undefined)
assert.equal(disposed, 1)

failNextCreate = true
await assert.rejects(
  runtimes.reset({
    agentId: 'agent-1',
    workspacePath: '/tmp/dsh-chaos-agent-1',
    provider: 'openai',
    model: 'broken',
    preset: 'default',
    sessionId: 'session-broken',
  }, reset.generation),
  /injected create failure/,
)
assert.equal(runtimes.resolve(reset), fakeAgent, 'failed replacement must recover the previous Session')

await assert.rejects(
  runtimes.reset({
    agentId: 'agent-1',
    workspacePath: '/tmp/dsh-chaos-agent-1',
    provider: 'openai',
    model: 'stale',
    preset: 'default',
    sessionId: 'session-stale',
  }, binding.generation),
  /runtime_generation_mismatch/,
)

const initial = await runtimes.reset({
  agentId: 'agent-2',
  workspacePath: '/tmp/dsh-chaos-agent-2',
  provider: 'openai',
  model: 'codex',
  preset: 'default',
  sessionId: 'session-agent-2',
})
assert.equal(initial.generation, '1')
await assert.rejects(
  runtimes.reset({
    agentId: 'agent-2',
    workspacePath: '/tmp/dsh-chaos-agent-2',
    provider: 'openai',
    model: 'codex',
    preset: 'default',
  }),
  /runtime_generation_mismatch/,
)
await runtimes.stop('agent-2')

const concurrent = await Promise.allSettled([
  runtimes.reset({
    agentId: 'agent-1',
    workspacePath: '/tmp/dsh-chaos-agent-1',
    provider: 'openai',
    model: 'codex-next',
    preset: 'default',
    sessionId: 'session-3a',
  }, reset.generation),
  runtimes.reset({
    agentId: 'agent-1',
    workspacePath: '/tmp/dsh-chaos-agent-1',
    provider: 'openai',
    model: 'codex-other',
    preset: 'default',
    sessionId: 'session-3b',
  }, reset.generation),
])
assert.equal(concurrent.filter(result => result.status === 'fulfilled').length, 1)
assert.equal(concurrent.filter(result => result.status === 'rejected').length, 1)
const replaced = concurrent.find(result => result.status === 'fulfilled').value
assert.equal(replaced.generation, '3')
assert.match(concurrent.find(result => result.status === 'rejected').reason.message, /runtime_generation_mismatch/)

await runtimes.stop('agent-1')
assert.equal(disposed, 5)
collab.bindings.set('agent-1', { ...replaced, preset: 'default' })
const resumed = await runtimes.resume('agent-1')
assert.equal(resumed.preset, 'standard')
assert.equal(runtimes.resolve(resumed), fakeAgent)
assert.deepEqual(mountedPresets, ['standard', 'standard', 'standard', 'standard', 'standard', 'standard'])
assert.deepEqual(sessionEvents, [{ type: 'agent-preset/selected', data: { agentPreset: 'standard' } }])
await runtimes.close()
assert.equal(disposed, 6)
assert.deepEqual(warningErrors, [])
console.log('runtime/delivery/tools smoke ok')
