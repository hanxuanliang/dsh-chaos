import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createRequire } from 'node:module'

const require = createRequire(import.meta.url)
const native = require('../native/dsh_chaos_core.node')
const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-'))

try {
  const core = await native.openCollab(join(root, 'state.db'))
  const owner = await core.createUser('owner', 'Owner')
  const alpha = await core.createAgent('alpha', 'Alpha', join(root, 'alpha'))
  const beta = await core.createAgent('beta', 'Beta', join(root, 'beta'))
  const channel = await core.createChannel('design', owner.id)
  await core.addMember(channel.id, alpha.id, owner.id)
  await core.addMember(channel.id, beta.id, owner.id)

  const sent = await core.sendMessage({
    targetId: channel.id,
    authorId: owner.id,
    clientRequestId: 'native-smoke-send',
    text: 'review this',
  })
  assert.equal(sent.recipientIds.length, 2)
  assert.equal(sent.wakeAgentIds.length, 2)

  const binding = await core.bindRuntime(alpha.id, 'alpha-session-1', 'openai', 'codex', 'default')
  assert.deepEqual(await core.runtimeBinding(alpha.id), binding)
  assert.deepEqual(await core.runtimeBindingForSession(binding.sessionId), binding)
  assert.equal((await core.listRuntimeBindings()).length, 1)
  const wakes = await core.listPendingWakes(20)
  assert.equal(wakes.length, 1)
  assert.equal(wakes[0].pendingSeq, sent.message.seq)
  await core.markNotified(alpha.id, binding.generation, binding.sessionId, wakes[0].pendingSeq)
  assert.equal((await core.listPendingWakes(20)).length, 0)
  await core.rearmRuntimeWake(alpha.id, binding.generation, binding.sessionId)
  assert.equal((await core.listPendingWakes(20)).length, 1)
  await core.markNotified(alpha.id, binding.generation, binding.sessionId, wakes[0].pendingSeq)

  assert.deepEqual(
    await core.readMessage(alpha.id, channel.id, sent.message.id),
    sent.message,
  )
  assert.equal((await core.readMessages(alpha.id, channel.id, '0', 20)).length, 1)
  const inbox = await core.checkInbox(alpha.id, binding.generation, binding.sessionId, 20)
  assert.equal(inbox.messages.length, 1)
  await core.markModelSeen(inbox.id, alpha.id, binding.generation, binding.sessionId)

  const task = await core.createTask(sent.message.id, owner.id)
  const claimed = await core.claimTask(task.messageId, alpha.id)
  assert.equal(claimed.status, 'in_progress')
  await assert.rejects(core.claimTask(task.messageId, beta.id), /task_already_claimed/)
  await core.close()
} finally {
  await rm(root, { recursive: true, force: true })
}
