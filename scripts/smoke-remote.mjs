import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Context } from '@deepseek-ai/cordis'
import CollabService from '../lib/index.js'

const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-remote-'))
const ctx = new Context()
let rpcRegistration
let sseRoute
const runtimeAgent = {
  id: 'unused-session',
  status: 'idle',
  followup: () => {},
  steer: () => {},
}
const dependencies = [
  ctx.provide('agentLoop', {}),
  ctx.provide('agents', {
    create: async () => ({ agent: runtimeAgent, dispose: async () => {} }),
    resume: async () => { throw new Error('unused') },
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
