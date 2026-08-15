import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Context } from '@deepseek-ai/cordis'
import CollabService from '../lib/index.js'

const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-service-'))
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
      options.setup({
        on: () => {},
        tools: { register: definition => { scopedTools.set(definition.name, definition) } },
      })
      return { agent: runtimeAgent, dispose: async () => {} }
    },
    resume: async () => { throw new Error('unused') },
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
    preset: 'default',
    sessionId: 'service-session',
  })
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
  const execution = {
    callId: 'service-tool-call',
    signal: new AbortController().signal,
    agent: runtimeAgent,
  }
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
  assert.equal(task.status, 'todo')
  assert.equal(claimed.assigneeId, alpha.id)

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
  await fiber.dispose()
} finally {
  for (const dispose of dependencies.reverse()) dispose()
  await rm(root, { recursive: true, force: true })
}
