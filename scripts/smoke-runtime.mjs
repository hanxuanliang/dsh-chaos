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
      checkedAtMs: 2,
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
let disposed = 0
const collab = new FakeCollab()
const fakeAgent = {
  id: 'session-1',
  status: 'idle',
  followup: message => notices.push(message),
  steer: message => steers.push(message),
}
const registry = {
  async create(options) {
    options.setup({
      on: () => {},
      tools: { register: definition => { tools.set(definition.name, definition) } },
    })
    collab.published = true
    fakeAgent.id = String(options.sessionId)
    return { agent: fakeAgent, dispose: async () => { disposed += 1 } }
  },
  async resume() { throw new Error('unused') },
}
const warningErrors = []
const warnings = { warn: (message, error) => warningErrors.push({ message, error }) }
const runtimes = new RuntimeManager(registry, collab, warnings)
const binding = await runtimes.create({
  agentId: 'agent-1',
  workspacePath: '/tmp/dsh-chaos-agent-1',
  provider: 'openai',
  model: 'codex',
  preset: 'default',
  sessionId: 'session-1',
})
assert.equal(runtimes.resolve(binding), fakeAgent)
assert.deepEqual([...tools.keys()].sort(), [
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
assert.equal(createdTask.status, 'todo')
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
assert.deepEqual(collab.taskActors, [
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
})
assert.equal(reset.generation, '2')
assert.equal(runtimes.resolve(binding), undefined)
assert.equal(disposed, 1)
await runtimes.close()
assert.equal(disposed, 2)
assert.deepEqual(warningErrors, [])
console.log('runtime/delivery/tools smoke ok')
