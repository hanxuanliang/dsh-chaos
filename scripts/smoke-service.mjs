import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Context } from '@deepseek-ai/cordis'
import CollabService from '../lib/index.js'

const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-service-'))
process.env.DSH_HOME = root
const ctx = new Context()
const scopedTools = new Map()
const runtimeAgent = {
  id: 'service-session',
  status: 'idle',
  followup: () => {},
  steer: () => {},
}
const dependencies = [
  ctx.provide('agentLoop', {}),
  ctx.provide('agents', {
    create: async (options) => {
      runtimeAgent.id = String(options.sessionId)
      await options.setup({
        on: () => {},
        systemPrompt: { section: () => () => {} },
        tools: { register: definition => { scopedTools.set(definition.name, definition) } },
      })
      return { agent: runtimeAgent, dispose: async () => {} }
    },
    resume: async () => { throw new Error('unused') },
  }),
  ctx.provide('agentPresets', {
    defaultId: 'standard',
    async list() { return [{ id: 'standard', trust: 'system', name: 'Standard' }] },
    async resolve(id) { return { id: id === undefined || id === 'default' ? 'standard' : id } },
    async mount(_agentCtx, id) { return { id } },
  }),
  ctx.provide('permissionPresets', {
    set(_session, preset) { assert.equal(preset, 'danger-full-access') },
  }),
  ctx.provide('tools', {}),
  ctx.provide('llm', {}),
]

try {
  const fiber = await ctx.plugin(CollabService, { path: join(root, 'state.db') })
  const owner = await ctx.collab.createUser('owner', 'Owner')
  const alpha = await ctx.collab.createAgent('alpha', 'Alpha', join(root, 'alpha'))
  const channel = await ctx.collab.createChannel('design', owner.id)
  await ctx.collab.addMember(channel.id, alpha.id, owner.id)
  const binding = await ctx.collab.createRuntime({
    agentId: alpha.id,
    workspacePath: join(root, 'alpha'),
    provider: 'openai',
    model: 'codex',
    preset: 'standard',
    sessionId: 'service-session',
  })
  assert.equal(binding.preset, 'standard')
  const sent = await ctx.collab.sendMessage({
    targetId: channel.id,
    authorId: owner.id,
    clientRequestId: 'service-smoke-send',
    text: 'hello',
  })
  assert.deepEqual(sent.wakeAgentIds, [alpha.id])
  const checked = await scopedTools.get('message_check').execute({ limit: 20 }, {
    callId: 'service-check',
    signal: new AbortController().signal,
    agent: runtimeAgent,
  })
  assert.equal(checked.messages.length, 1)
  assert.equal(checked.contexts[0].target.name, 'design')
  assert.equal(checked.contexts[0].members.find(member => member.actor.id === alpha.id).actor.handle, 'alpha')
  const execution = {
    callId: 'service-tool-call',
    signal: new AbortController().signal,
    agent: runtimeAgent,
  }
  const identity = await scopedTools.get('identity_context').execute({ targetId: channel.id }, execution)
  assert.equal(identity.agent.actor.handle, 'alpha')
  assert.equal(identity.members.find(member => member.actor.id === owner.id).role, 'owner')
  const history = await scopedTools.get('message_read').execute({
    targetId: channel.id,
    afterSeq: '0',
    limit: 20,
  }, execution)
  assert.equal(history.messages[0].id, sent.message.id)
  const task = await scopedTools.get('task_create').execute({
    messageId: sent.message.id,
  }, execution)
  const claimed = await scopedTools.get('task_claim').execute({
    messageId: sent.message.id,
  }, execution)
  // 方案A(2026-08-19): task_create = create+claim — 创建即自领, in_progress
  assert.equal(task.status, 'in_progress')
  assert.equal(claimed.assigneeId, alpha.id)
  const reviewed = await scopedTools.get('task_update').execute({
    messageId: sent.message.id,
    status: 'in_review',
    expectedVersion: claimed.version,
  }, execution)
  const listed = await scopedTools.get('task_list').execute({ targetId: channel.id }, execution)
  assert.equal(listed.tasks[0].status, 'in_review')
  const unclaimed = await scopedTools.get('task_unclaim').execute({
    messageId: sent.message.id,
    expectedVersion: reviewed.version,
  }, execution)
  assert.equal(unclaimed.assigneeId, undefined)

  const request = {
    provider: 'openai',
    model: 'codex',
    messages: [],
    sessionId: binding.sessionId,
  }
  const stream = ctx.waterfall(ctx, 'llm/stream', request, () => (async function* () {})())
  for await (const _chunk of stream) {}
  const empty = await ctx.collab.checkInbox(alpha.id, binding.generation, binding.sessionId, 20)
  assert.equal(empty.messages.length, 0)
  assert.deepEqual(empty.contexts, [])

  const named = await ctx.collab.createConfiguredAgent(
    'Alpha Two',
    'alpha-two',
    'Own the second lane',
    'openai',
    'codex',
    'standard',
  )
  assert.equal(named.profile.actor.displayName, 'Alpha Two')
  assert.equal(named.profile.charter.summary, 'Own the second lane')
  assert.equal(named.setupError, undefined)
  assert.match(named.profile.workspacePath, /\/agents\/[0-9a-f-]{36}$/)
  const { stat } = await import('node:fs/promises')
  assert.equal((await stat(named.profile.workspacePath)).isDirectory(), true)
  await fiber.dispose()
} finally {
  for (const dispose of dependencies.reverse()) dispose()
  await rm(root, { recursive: true, force: true })
}
