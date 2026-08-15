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
