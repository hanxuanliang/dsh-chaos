import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Context } from '@deepseek-ai/cordis'
import { apply as connectionApply, inject as connectionInject } from '@deepseek-ai/dsh-client-connection'
import WebServer from '@deepseek-ai/dsh-host-webserver'
import CollabService from '../lib/index.js'

const root = await mkdtemp(join(tmpdir(), 'dsh-chaos-transport-'))
const ctx = new Context()
const dependencies = [
  ctx.provide('agentLoop', {}),
  ctx.provide('agents', {
    create: async () => { throw new Error('unused') },
    resume: async () => { throw new Error('unused') },
  }),
  ctx.provide('agentPresets', {
    defaultId: 'standard',
    async list() { return [{ id: 'standard', trust: 'system' }] },
    async resolve(id) { return { id: id ?? 'standard' } },
    async mount(_agentCtx, id) { return { id } },
  }),
  ctx.provide('permissionPresets', { set: () => {} }),
  ctx.provide('tools', {}),
  ctx.provide('llm', {}),
]
const fibers = []

try {
  fibers.push(await ctx.plugin(WebServer, { host: '127.0.0.1', port: 0 }))
  fibers.push(await ctx.plugin({ apply: connectionApply, inject: [...connectionInject] }, {}))
  fibers.push(await ctx.plugin(CollabService, {
    path: join(root, 'state.db'),
    webUserHandle: 'transport-owner',
    webUserDisplayName: 'Transport Owner',
    sseHeartbeatMs: 1_000,
  }))
  const base = `http://127.0.0.1:${String(ctx.webServer.port)}`
  let rpcCounter = 0
  const call = async (endpoint, payload, headers = {}) => {
    rpcCounter += 1
    const response = await fetch(`${base}/dsh-chaos/${endpoint}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...headers },
      body: JSON.stringify({
        type: 'client-request',
        rpcId: `transport-${String(rpcCounter)}`,
        method: endpoint,
        payload,
      }),
    })
    return { response, body: response.status === 200 ? await response.json() : undefined }
  }

  const bootstrap = await call('snapshot', {})
  assert.equal(bootstrap.response.status, 200)
  assert.equal(bootstrap.body.result.ok, true)
  assert.equal(bootstrap.body.result.value.ok, true)
  const snapshot = bootstrap.body.result.value.value
  assert.equal(snapshot.actor.handle, 'transport-owner')

  const blocked = await call('snapshot', {}, {
    origin: 'http://attacker.example',
    'sec-fetch-site': 'cross-site',
  })
  assert.equal(blocked.response.status, 403)

  const abort = new AbortController()
  const events = await fetch(`${base}/dsh-chaos/events?cursor=0`, {
    headers: { 'last-event-id': snapshot.cursor },
    signal: abort.signal,
  })
  assert.equal(events.status, 200)
  assert.match(events.headers.get('content-type') ?? '', /^text\/event-stream/)
  const reader = events.body.getReader()
  const decoder = new TextDecoder()
  let wire = ''
  const readUntilChange = (async () => {
    while (!wire.includes('event: change')) {
      const chunk = await reader.read()
      if (chunk.done) break
      wire += decoder.decode(chunk.value, { stream: true })
    }
  })()
  const created = await call('channel.create', {
    name: 'transport-channel',
    description: 'Transport collaboration',
  })
  assert.equal(created.body.result.value.ok, true)
  await readUntilChange
  assert.match(wire, /event: change/)
  assert(wire.includes(`"entityId":"${created.body.result.value.value.id}"`))
  abort.abort()
  await reader.cancel().catch(() => {})

  const sent = await call('message.send', {
    targetId: created.body.result.value.value.id,
    requestId: 'transport-activity',
    text: 'transport Activity',
  })
  assert.equal(sent.body.result.value.ok, true)
  const activity = await call('inbox.list', {})
  assert.equal(activity.body.result.value.ok, true)
  assert.equal(activity.body.result.value.value.activeCount, '1')
  assert.equal(activity.body.result.value.value.items[0].title, 'transport Activity')
  const done = await call('inbox.done', {
    targetId: created.body.result.value.value.id,
    throughSeq: activity.body.result.value.value.items[0].lastActivitySeq,
  })
  assert.equal(done.body.result.value.ok, true)
  assert.equal((await call('inbox.list', {})).body.result.value.value.activeCount, '0')

  const retentionFloor = await ctx.collab.pruneChangesBefore(Date.now() + 1)
  const resync = await fetch(`${base}/dsh-chaos/events?cursor=0`, {
    headers: { 'last-event-id': '0' },
  })
  assert.equal(resync.status, 200)
  const resyncWire = await resync.text()
  assert.match(resyncWire, /event: resync_required/)
  assert.match(resyncWire, /"code":"change_cursor_resync_required"/)
  const recovered = await call('snapshot', {})
  assert.equal(recovered.body.result.value.value.cursor, retentionFloor)
} finally {
  for (const fiber of fibers.reverse()) await fiber.dispose()
  for (const dispose of dependencies.reverse()) dispose()
  await rm(root, { recursive: true, force: true })
}
