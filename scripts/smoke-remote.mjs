import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Context } from '@deepseek-ai/cordis'
import CollabService from '../lib/index.js'

const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-remote-'))
process.env.DSH_HOME = root
const ctx = new Context()
let rpcRegistration
let sseRoute
const promptSections = []
const runtimeAgent = {
  id: 'unused-session',
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
        systemPrompt: {
          section: definition => {
            promptSections.push(definition)
            return () => {}
          },
        },
        tools: { register: () => {} },
      })
      return { agent: runtimeAgent, dispose: async () => {} }
    },
    resume: async () => { throw new Error('unused') },
  }),
  ctx.provide('agentPresets', {
    defaultId: 'standard',
    async list() {
      return [
        { id: 'standard', trust: 'system', name: 'Standard' },
        { id: 'code', trust: 'system', name: 'PTC' },
        { id: 'cordis', trust: 'system', name: 'Cordis' },
        { id: 'minimal', trust: 'system', name: 'Minimal', description: 'Small tool set' },
      ]
    },
    async resolve(id) { return { id: id ?? 'standard' } },
    async mount(_agentCtx, id) { return { id } },
  }),
  ctx.provide('tools', {}),
  ctx.provide('llm', {}),
  ctx.provide('connection', {
    rpc: {
      handle(channel, handler, options) {
        rpcRegistration = { channel, handler, options }
        return async () => { rpcRegistration = undefined }
      },
    },
  }),
  ctx.provide('webServer', {
    register(route) {
      sseRoute = route
      return () => { sseRoute = undefined }
    },
  }),
]

class MockResponse extends EventEmitter {
  statusCode = 0
  headers = {}
  chunks = []
  destroyed = false
  writableEnded = false

  writeHead(statusCode, headers = {}) {
    this.statusCode = statusCode
    this.headers = headers
    return this
  }

  flushHeaders() {}

  write(chunk) {
    this.chunks.push(String(chunk))
    return true
  }

  end(chunk) {
    if (chunk !== undefined) this.chunks.push(String(chunk))
    this.writableEnded = true
    return this
  }

  close() {
    this.destroyed = true
    this.emit('close')
  }
}

async function until(predicate, description) {
  const deadline = Date.now() + 2_000
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${description}`)
    await new Promise(resolve => setTimeout(resolve, 5))
  }
}

try {
  const fiber = await ctx.plugin(CollabService, {
    path: join(root, 'state.db'),
    webUserHandle: 'browser-owner',
    webUserDisplayName: 'Browser Owner',
    sseHeartbeatMs: 1_000,
  })
  assert.equal(rpcRegistration.channel, '/dsh-chaos')
  assert.equal(rpcRegistration.options.authority, 'loopback')
  assert.equal(sseRoute.kind, 'exact')
  assert.equal(sseRoute.path, '/dsh-chaos/events')

  const call = async (endpoint, payload) => {
    const transport = await rpcRegistration.handler(
      endpoint,
      payload,
      new AbortController().signal,
    )
    assert.equal(transport.ok, true)
    return transport.value
  }
  const firstSnapshot = await call('snapshot', {})
  assert.equal(firstSnapshot.ok, true)
  assert.equal(firstSnapshot.value.actor.handle, 'browser-owner')

  const presets = await call('agent.presets', {})
  assert.equal(presets.ok, true)
  assert.equal(presets.value[0].isDefault, true)
  assert.deepEqual(presets.value.map(preset => preset.id), ['standard', 'code', 'cordis'])
  const unsupportedCreate = await call('agent.create', {
    displayName: 'Unsupported Agent',
    handle: 'unsupported-agent',
    description: 'Must not be created',
    provider: 'openai',
    model: 'codex',
    presetId: 'minimal',
  })
  assert.equal(unsupportedCreate.ok, false)
  assert.equal(unsupportedCreate.error.code, 'invalid_argument')
  const createdAgent = await call('agent.create', {
    displayName: 'Workspace Agent',
    handle: 'workspace-agent',
    description: 'Own workspace verification',
    provider: 'openai',
    model: 'codex',
    presetId: 'code',
  })
  assert.equal(createdAgent.ok, true)
  assert.equal(createdAgent.value.profile.binding.preset, 'code')
  const agentId = createdAgent.value.profile.actor.id
  const profile = await call('agent.profile', { agentId })
  assert.equal(profile.ok, true)
  assert.equal(profile.value.binding.sessionId, createdAgent.value.profile.binding.sessionId)
  assert.equal(profile.value.charter.summary, 'Own workspace verification')
  assert.equal(profile.value.workspacePath, join(root, 'agents', agentId))
  const identityPrompt = promptSections.at(-1)
  assert(identityPrompt)
  assert.match(identityPrompt.text(), /You are Workspace Agent \(@workspace-agent\)/)
  assert.match(identityPrompt.text(), /Own workspace verification/)
  assert.match(identityPrompt.text(), /message_check/)
  assert.equal(identityPrompt.text().includes(profile.value.workspacePath), true)
  const agentsSeed = await readFile(join(profile.value.workspacePath, 'AGENTS.md'), 'utf8')
  const memorySeed = await readFile(join(profile.value.workspacePath, 'MEMORY.md'), 'utf8')
  assert.match(agentsSeed, /layout is intentionally unspecified/)
  assert.match(memorySeed, /# Workspace Agent/)
  assert.match(memorySeed, /## Key Knowledge/)
  const profiles = await call('agent.profiles', {})
  assert.equal(profiles.ok, true)
  assert(profiles.value.some(item => item.actor.id === agentId && item.binding.sessionId === profile.value.binding.sessionId))
  const updatedProfile = await call('agent.profile.update', {
    agentId,
    displayName: 'Workspace Reviewer',
    description: 'Review workspace behavior',
    expectedProfileVersion: profile.value.profileVersion,
  })
  assert.equal(updatedProfile.ok, true)
  assert.equal(updatedProfile.value.actor.displayName, 'Workspace Reviewer')
  assert.equal(updatedProfile.value.charter.summary, 'Review workspace behavior')
  assert.match(identityPrompt.text(), /You are Workspace Reviewer \(@workspace-agent\)/)
  assert.match(identityPrompt.text(), /Review workspace behavior/)
  const avatar = await call('agent.avatar.update', {
    agentId,
    avatarDataUrl: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
    expectedProfileVersion: updatedProfile.value.profileVersion,
  })
  assert.equal(avatar.ok, true)
  assert.equal(avatar.value.actor.avatarDataUrl.startsWith('data:image/png;base64,'), true)
  const staleProfile = await call('agent.profile.update', {
    agentId,
    displayName: 'Stale',
    description: 'Must not overwrite',
    expectedProfileVersion: profile.value.profileVersion,
  })
  assert.equal(staleProfile.ok, false)
  assert.equal(staleProfile.error.code, 'agent_profile_version_conflict')
  await writeFile(join(profile.value.workspacePath, 'hello.txt'), 'hello workspace')
  await writeFile(join(profile.value.workspacePath, 'binary.bin'), Buffer.from([0, 1, 2]))
  await writeFile(join(profile.value.workspacePath, 'large.txt'), 'x'.repeat(512 * 1024 + 1))
  await mkdir(join(profile.value.workspacePath, '.hidden'))
  await symlink('/etc/passwd', join(profile.value.workspacePath, 'escape'))
  const workspace = await call('agent.workspace.list', {
    agentId,
    dirPath: '',
    includeHidden: false,
  })
  assert.equal(workspace.ok, true)
  assert.deepEqual(
    workspace.value.map(entry => entry.name),
    ['AGENTS.md', 'binary.bin', 'hello.txt', 'large.txt', 'MEMORY.md', 'escape'],
  )
  assert.equal(workspace.value[5].kind, 'symlink')
  const hiddenWorkspace = await call('agent.workspace.list', {
    agentId,
    dirPath: '',
    includeHidden: true,
  })
  assert.equal(hiddenWorkspace.value[0].name, '.hidden')
  const preview = await call('agent.workspace.read', {
    agentId,
    path: 'hello.txt',
  })
  assert.equal(preview.ok, true)
  assert.equal(preview.value.content, 'hello workspace')
  const binaryPreview = await call('agent.workspace.read', {
    agentId,
    path: 'binary.bin',
  })
  assert.equal(binaryPreview.value.binary, true)
  assert.equal(binaryPreview.value.content, undefined)
  const largePreview = await call('agent.workspace.read', {
    agentId,
    path: 'large.txt',
  })
  assert.equal(largePreview.value.truncated, true)
  assert.equal(largePreview.value.content.length, 512 * 1024)
  const escapedPreview = await call('agent.workspace.read', {
    agentId,
    path: '../outside.txt',
  })
  assert.equal(escapedPreview.ok, false)
  assert.equal(escapedPreview.error.code, 'invalid_argument')
  const symlinkPreview = await call('agent.workspace.read', {
    agentId,
    path: 'escape',
  })
  assert.equal(symlinkPreview.ok, false)
  assert.equal(symlinkPreview.error.code, 'invalid_argument')

  // Runtime replacement is generation-fenced; only the exact binding can restart.
  const paired = await call('agent.create', {
    displayName: 'Paired Agent',
    handle: 'paired-agent',
    description: 'Own pairing checks',
    provider: 'openai',
    model: 'codex',
    presetId: 'standard',
  })
  assert.equal(paired.ok, true)
  assert.equal(paired.value.profile.binding.provider, 'openai')
  await writeFile(
    join(paired.value.profile.workspacePath, 'AGENTS.md'),
    '# Agent-owned instructions\n',
  )
  await writeFile(
    join(paired.value.profile.workspacePath, 'MEMORY.md'),
    '# Agent-owned memory\n',
  )
  const unsupportedReset = await call('agent.runtime.replace', {
    agentId: paired.value.profile.actor.id,
    provider: 'openai',
    model: 'codex-next',
    presetId: 'minimal',
    expectedGeneration: paired.value.profile.binding.generation,
  })
  assert.equal(unsupportedReset.ok, false)
  assert.equal(unsupportedReset.error.code, 'invalid_argument')
  const replaced = await call('agent.runtime.replace', {
    agentId: paired.value.profile.actor.id,
    provider: 'openai',
    model: 'codex-next',
    presetId: 'cordis',
    expectedGeneration: paired.value.profile.binding.generation,
  })
  assert.equal(replaced.ok, true)
  assert.equal(replaced.value.generation, '2')
  assert.equal(
    await readFile(join(paired.value.profile.workspacePath, 'AGENTS.md'), 'utf8'),
    '# Agent-owned instructions\n',
  )
  assert.equal(
    await readFile(join(paired.value.profile.workspacePath, 'MEMORY.md'), 'utf8'),
    '# Agent-owned memory\n',
  )
  const staleRuntime = await call('agent.runtime.replace', {
    agentId: paired.value.profile.actor.id,
    provider: 'openai',
    model: 'stale',
    presetId: 'standard',
    expectedGeneration: paired.value.profile.binding.generation,
  })
  assert.equal(staleRuntime.ok, false)
  assert.equal(staleRuntime.error.code, 'runtime_generation_mismatch')
  const partial = await call('agent.create', {
    displayName: 'Missing Model',
    handle: 'missing-model',
    description: 'Invalid request',
    provider: 'openai',
    presetId: 'standard',
  })
  assert.equal(partial.ok, false)
  assert.equal(partial.error.code, 'invalid_argument')
  const invalidHandle = await call('agent.create', {
    displayName: 'Invalid Handle',
    handle: 'Invalid Handle',
    description: 'Must be rejected',
    provider: 'openai',
    model: 'codex',
    presetId: 'standard',
  })
  assert.equal(invalidHandle.ok, false)
  assert.equal(invalidHandle.error.code, 'invalid_argument')

  const unconfiguredActor = await ctx.collab.createAgent('unconfigured', 'Unconfigured', join(root, 'agents', 'unconfigured'))
  const profilesWithUnconfigured = await call('agent.profiles', {})
  assert(profilesWithUnconfigured.value.some(item => item.actor.id === unconfiguredActor.id && item.binding === undefined))

  const created = await call('channel.create', {
    name: 'remote-channel',
    creatorId: 'forged-actor',
  })
  assert.equal(created.ok, true)
  assert.equal(created.value.createdBy, firstSnapshot.value.actor.id)
  const snapshot = await call('snapshot', {})
  assert.equal(snapshot.ok, true)
  assert(snapshot.value.targets.some(target => target.id === created.value.id))

  // history.tail: exact count plus the true latest page in one RPC.
  for (let index = 0; index < 3; index += 1) {
    const sent = await call('message.send', {
      targetId: created.value.id,
      requestId: `tail-${String(index)}`,
      text: `tail message ${String(index)}`,
    })
    assert.equal(sent.ok, true)
  }
  const tail = await call('history.tail', { targetId: created.value.id, limit: 2 })
  assert.equal(tail.ok, true)
  assert.equal(tail.value.count, '3')
  assert.equal(tail.value.messages.length, 2)
  assert.equal(tail.value.messages[0].text, 'tail message 1')
  assert.equal(tail.value.messages[1].text, 'tail message 2')
  const badTail = await call('history.tail', { targetId: created.value.id, limit: 0 })
  assert.equal(badTail.ok, false)
  assert.equal(badTail.error.code, 'invalid_argument')

  // target.members: the membership projection, not the actor directory.
  const members = await call('target.members', { targetId: created.value.id })
  assert.equal(members.ok, true)
  assert.equal(members.value.length, 1)
  assert.equal(members.value[0].id, firstSnapshot.value.actor.id)
  const otherUser = await call('actors', {})
  assert.equal(otherUser.ok, true)
  const badMembers = await call('target.members', { targetId: 'missing-target' })
  assert.equal(badMembers.ok, false)

  // task.create carries the authoritative anchor text from the store.
  const anchored = await call('task.create', { messageId: tail.value.messages[0].id })
  assert.equal(anchored.ok, true)
  assert.equal(anchored.value.anchorText, 'tail message 1')
  const tasks = await call('tasks', { targetId: created.value.id })
  assert.equal(tasks.ok, true)
  assert.equal(tasks.value.length, 1)
  assert.equal(tasks.value[0].anchorText, 'tail message 1')

  const activity = await call('inbox.list', { limit: 20 })
  assert.equal(activity.ok, true)
  assert.equal(activity.value.activeCount, '1')
  assert.equal(activity.value.items[0].conversationId, created.value.id)
  assert.equal(activity.value.items[0].title, 'tail message 2')
  const malformedCursor = await call('inbox.list', { cursor: 'bad-cursor' })
  assert.equal(malformedCursor.ok, false)
  assert.equal(malformedCursor.error.code, 'invalid_argument')
  const futureDone = await call('inbox.done', {
    targetId: created.value.id,
    throughSeq: '9223372036854775807',
  })
  assert.equal(futureDone.ok, false)
  assert.equal(futureDone.error.code, 'invalid_argument')
  const done = await call('inbox.done', {
    targetId: created.value.id,
    throughSeq: activity.value.items[0].lastActivitySeq,
  })
  assert.equal(done.ok, true)
  assert.equal((await call('inbox.list', {})).value.activeCount, '0')
  const revived = await call('message.send', {
    targetId: created.value.id,
    requestId: 'activity-revive',
    text: 'newer Activity',
  })
  assert.equal(revived.ok, true)
  assert.equal((await call('inbox.list', {})).value.activeCount, '1')

  // agent.delete: the Agent leaves the actor directory and loses its binding,
  // while its history messages keep pointing at the retained actors row.
  const historyChannel = await call('channel.create', { name: 'delete-history' })
  assert.equal(historyChannel.ok, true)
  const added = await call('member.add', {
    targetId: historyChannel.value.id,
    memberId: paired.value.profile.actor.id,
  })
  assert.equal(added.ok, true)
  const memberships = await call('agent.memberships', { agentId: paired.value.profile.actor.id })
  assert.equal(memberships.ok, true)
  assert(memberships.value.some(item => item.target.id === historyChannel.value.id && item.role === 'member'))
  const mentionRoot = await call('message.send', {
    targetId: historyChannel.value.id,
    requestId: 'mention-root',
    text: 'Open a mention Thread',
  })
  assert.equal(mentionRoot.ok, true)
  const mentionThread = await call('thread.create', { rootMessageId: mentionRoot.value.message.id })
  assert.equal(mentionThread.ok, true)
  const rootInbox = await ctx.collab.checkInbox(
    paired.value.profile.actor.id,
    replaced.value.generation,
    replaced.value.sessionId,
    10,
  )
  assert.equal(rootInbox.messages.length, 1)
  await ctx.collab.markModelSeen(
    rootInbox.id,
    paired.value.profile.actor.id,
    replaced.value.generation,
    replaced.value.sessionId,
  )
  assert.equal(
    (await ctx.collab.snapshot(paired.value.profile.actor.id)).followedThreadIds.length,
    0,
  )
  const mentioned = await call('message.send', {
    targetId: mentionThread.value.id,
    requestId: 'mention-agent-without-follow',
    text: '@paired-agent please review',
  })
  assert.equal(mentioned.ok, true)
  assert.deepEqual(mentioned.value.wakeAgentIds, [paired.value.profile.actor.id])
  const mentionInbox = await ctx.collab.checkInbox(
    paired.value.profile.actor.id,
    replaced.value.generation,
    replaced.value.sessionId,
    10,
  )
  assert.equal(mentionInbox.messages.length, 1)
  assert.equal(mentionInbox.messages[0].message.id, mentioned.value.message.id)
  assert.deepEqual(
    (await ctx.collab.snapshot(paired.value.profile.actor.id)).followedThreadIds,
    [mentionThread.value.id],
  )
  const pairedNote = await ctx.collab.sendMessage({
    targetId: historyChannel.value.id,
    authorId: paired.value.profile.actor.id,
    clientRequestId: 'paired-note',
    text: 'paired was here',
  })
  assert.equal(pairedNote.message.authorId, paired.value.profile.actor.id)
  const deleted = await call('agent.delete', { agentId: paired.value.profile.actor.id })
  assert.equal(deleted.ok, true)
  const actorsAfterDelete = await call('actors', {})
  assert(!actorsAfterDelete.value.some(actor => actor.id === paired.value.profile.actor.id))
  assert(actorsAfterDelete.value.some(actor => actor.id === agentId))
  const bindingsAfterDelete = await call('runtime.bindings', {})
  assert(!bindingsAfterDelete.value.some(binding => binding.agentId === paired.value.profile.actor.id))
  const history = await call('history', { targetId: historyChannel.value.id })
  assert.equal(history.ok, true)
  assert(history.value.some(
    message => message.authorId === paired.value.profile.actor.id && message.text === 'paired was here',
  ))
  const deletedProfile = await call('agent.profile', { agentId: paired.value.profile.actor.id })
  assert.equal(deletedProfile.ok, false)
  assert.equal(deletedProfile.error.code, 'not_found')
  const deletedAgain = await call('agent.delete', { agentId: paired.value.profile.actor.id })
  assert.equal(deletedAgain.ok, true)
  const deleteUser = await call('agent.delete', { agentId: firstSnapshot.value.actor.id })
  assert.equal(deleteUser.ok, false)
  assert.equal(deleteUser.error.code, 'not_found')
  const deleteMissing = await call('agent.delete', { agentId: 'missing-agent' })
  assert.equal(deleteMissing.ok, false)
  assert.equal(deleteMissing.error.code, 'not_found')

  const response = new MockResponse()
  const request = {
    method: 'GET',
    url: '/dsh-chaos/events?cursor=0',
    headers: {
      host: '127.0.0.1:3080',
      origin: 'http://127.0.0.1:3080',
      'sec-fetch-site': 'same-origin',
      'last-event-id': snapshot.value.cursor,
    },
  }
  const stream = sseRoute.handler(request, response)
  await until(() => response.chunks.join('').includes(': connected'), 'SSE open')
  const second = await call('channel.create', { name: 'after-reconnect' })
  assert.equal(second.ok, true)
  await until(
    () => response.chunks.join('').includes(`"entityId":"${second.value.id}"`),
    'durable change event',
  )
  const wire = response.chunks.join('')
  assert.match(wire, /event: change/)
  assert.match(wire, /id: [1-9][0-9]*/)
  response.close()
  await stream
  assert.equal(response.statusCode, 200)

  const retentionFloor = await ctx.collab.pruneChangesBefore(Date.now() + 1)
  const resync = new MockResponse()
  await sseRoute.handler({
    method: 'GET',
    url: '/dsh-chaos/events?cursor=0',
    headers: {
      host: '127.0.0.1:3080',
      origin: 'http://127.0.0.1:3080',
      'sec-fetch-site': 'same-origin',
      'last-event-id': '0',
    },
  }, resync)
  const resyncWire = resync.chunks.join('')
  assert.match(resyncWire, /event: resync_required/)
  assert.match(resyncWire, /"code":"change_cursor_resync_required"/)
  assert.equal((await call('snapshot', {})).value.cursor, retentionFloor)

  const forbidden = new MockResponse()
  await sseRoute.handler({
    method: 'GET',
    url: '/dsh-chaos/events',
    headers: { host: 'attacker.example', 'sec-fetch-site': 'cross-site' },
  }, forbidden)
  assert.equal(forbidden.statusCode, 403)

  await fiber.dispose()
  assert.equal(rpcRegistration, undefined)
  assert.equal(sseRoute, undefined)
} finally {
  for (const dispose of dependencies.reverse()) dispose()
  await rm(root, { recursive: true, force: true })
}
