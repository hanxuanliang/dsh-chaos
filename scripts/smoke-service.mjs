import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Context } from '@deepseek-ai/cordis'
import CollabService from '../lib/index.js'

const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-service-'))
const ctx = new Context()

try {
  const fiber = await ctx.plugin(CollabService, { path: join(root, 'state.db') })
  const owner = await ctx.collab.createUser('owner', 'Owner')
  const alpha = await ctx.collab.createAgent('alpha', 'Alpha', join(root, 'alpha'))
  const channel = await ctx.collab.createChannel('design', owner.id)
  await ctx.collab.addMember(channel.id, alpha.id, owner.id)
  const sent = await ctx.collab.sendMessage({
    targetId: channel.id,
    authorId: owner.id,
    clientRequestId: 'service-smoke-send',
    text: 'hello',
  })
  assert.deepEqual(sent.wakeAgentIds, [alpha.id])
  await fiber.dispose()
} finally {
  await rm(root, { recursive: true, force: true })
}
