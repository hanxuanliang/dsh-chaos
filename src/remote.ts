import type { IncomingMessage, ServerResponse } from 'node:http'
import type { Context } from '@deepseek-ai/cordis'
import type { ConnectionRpcHandler } from '@deepseek-ai/dsh-client-connection'
import type {} from '@deepseek-ai/dsh-host-webserver'
import type {
  AgentPresetSummary,
  AgentMembership,
  AgentProfile,
  CreatedAgent,
  AgentWorkspaceEntry,
  AgentWorkspaceFile,
} from './agent-settings-types.ts'
import type {
  NativeActor,
  NativeActivityInboxPage,
  NativeChangeEvent,
  NativeCollabSnapshot,
  NativeMessage,
  NativeMessageTail,
  NativeRuntimeBinding,
  NativeSendResult,
  NativeTarget,
  NativeThreadSummary,
  NativeTask,
} from './native.ts'

export const COLLAB_RPC_CHANNEL = '/dsh-chaos'
export const COLLAB_EVENTS_PATH = '/dsh-chaos/events'

export interface CollabRemoteApi {
  listActors(actorId: string): Promise<NativeActor[]>
  listTargetMembers(actorId: string, targetId: string): Promise<NativeActor[]>
  snapshot(actorId: string): Promise<NativeCollabSnapshot>
  listChanges(actorId: string, afterSeq: string, limit: number): Promise<NativeChangeEvent[]>
  readMessages(actorId: string, targetId: string, afterSeq: string, limit: number): Promise<NativeMessage[]>
  readMessagesTail(actorId: string, targetId: string, limit: number): Promise<NativeMessageTail>
  inboxList(actorId: string, limit: number, cursor?: string, filter?: 'all' | 'unread'): Promise<NativeActivityInboxPage>
  inboxDone(actorId: string, targetId: string, throughSeq: string): Promise<void>
  inboxDoneAll(actorId: string): Promise<number>
  listTasks(actorId: string, targetId?: string): Promise<NativeTask[]>
  listAgentPresets(): Promise<AgentPresetSummary[]>
  agentProfile(viewerId: string, agentId: string): Promise<AgentProfile>
  agentProfiles(viewerId: string): Promise<AgentProfile[]>
  updateAgentProfile(
    viewerId: string,
    agentId: string,
    displayName: string,
    description: string,
    expectedProfileVersion: string,
  ): Promise<AgentProfile>
  replaceAgentRuntime(
    viewerId: string,
    agentId: string,
    provider: string,
    model: string,
    presetId: string,
    expectedGeneration?: string,
  ): Promise<NativeRuntimeBinding>
  agentMemberships(viewerId: string, agentId: string): Promise<AgentMembership[]>
  agentWorkspace(
    viewerId: string,
    agentId: string,
    dirPath: string,
    includeHidden: boolean,
  ): Promise<AgentWorkspaceEntry[]>
  agentWorkspaceFile(viewerId: string, agentId: string, path: string): Promise<AgentWorkspaceFile>
  createConfiguredAgent(
    displayName: string,
    handle: string,
    description: string,
    provider: string,
    model: string,
    presetId: string,
  ): Promise<CreatedAgent>
  deleteAgent(agentId: string): Promise<void>
  listRuntimeBindings(): Promise<NativeRuntimeBinding[]>
  createChannel(name: string, creatorId: string): Promise<NativeTarget>
  createDirect(actorId: string, peerId: string): Promise<NativeTarget>
  createThread(rootMessageId: string, actorId: string): Promise<NativeTarget>
  threadSummaries(actorId: string, rootMessageIds: string[]): Promise<NativeThreadSummary[]>
  followThread(threadTargetId: string, actorId: string): Promise<void>
  unfollowThread(threadTargetId: string, actorId: string): Promise<void>
  addMember(targetId: string, actorId: string, addedBy: string): Promise<void>
  sendMessage(input: {
    targetId: string
    authorId: string
    clientRequestId: string
    text: string
  }): Promise<NativeSendResult>
  createTask(messageId: string, actorId: string): Promise<NativeTask>
  claimTask(messageId: string, actorId: string): Promise<NativeTask>
  unclaimTask(messageId: string, actorId: string, expectedVersion: string): Promise<NativeTask>
  updateTaskStatus(
    messageId: string,
    actorId: string,
    status: NativeTask['status'],
    expectedVersion: string,
  ): Promise<NativeTask>
  onChange(listener: () => void): () => void
}

export interface CollabRemoteConfig {
  heartbeatMs: number
}

export interface CollabDomainError {
  code: string
  message: string
}

type CollabDomainFailure = { ok: false; error: CollabDomainError }

export type CollabDomainResult<T> =
  | { ok: true; value: T }
  | CollabDomainFailure

const DECIMAL = /^(0|[1-9][0-9]*)$/
const STATUS = new Set<NativeTask['status']>(['todo', 'in_progress', 'in_review', 'done'])

function success<T>(value: T): CollabDomainResult<T> {
  return { ok: true, value }
}

function failure(code: string, message: string): CollabDomainFailure {
  return { ok: false, error: { code, message } }
}

function errorResult(error: unknown): CollabDomainFailure {
  const message = error instanceof Error ? error.message : String(error)
  const match = /^\[([a-z0-9_]+)\]\s*/.exec(message)
  return failure(match?.[1] ?? 'internal', match === null ? message : message.slice(match[0].length))
}

function record(payload: unknown): Record<string, unknown> {
  if (payload === null || typeof payload !== 'object' || Array.isArray(payload)) {
    throw new Error('[invalid_argument] payload must be an object')
  }
  return payload as Record<string, unknown>
}

/** Non-empty string array with an upper bound (batch endpoints). */
function stringArray(input: Record<string, unknown>, key: string, max: number): string[] {
  const raw = input[key]
  if (!Array.isArray(raw) || raw.length === 0 || raw.length > max) {
    throw new Error(`[invalid_argument] ${key} must be an array of 1..${max} strings`)
  }
  const out: string[] = []
  for (const value of raw) {
    if (typeof value !== 'string' || value === '') {
      throw new Error(`[invalid_argument] ${key} entries must be non-empty strings`)
    }
    out.push(value)
  }
  return out
}

function requiredString(payload: Record<string, unknown>, name: string): string {
  const value = payload[name]
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`[invalid_argument] ${name} must be a non-empty string`)
  }
  return value
}

function optionalString(payload: Record<string, unknown>, name: string): string | undefined {
  const value = payload[name]
  if (value === undefined) return undefined
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`[invalid_argument] ${name} must be a non-empty string when provided`)
  }
  return value
}

function pathString(payload: Record<string, unknown>, name: string): string {
  const value = payload[name] ?? ''
  if (typeof value !== 'string') throw new Error(`[invalid_argument] ${name} must be a string`)
  return value
}

function booleanValue(payload: Record<string, unknown>, name: string, fallback: boolean): boolean {
  const value = payload[name] ?? fallback
  if (typeof value !== 'boolean') throw new Error(`[invalid_argument] ${name} must be a boolean`)
  return value
}

function decimalString(payload: Record<string, unknown>, name: string, fallback?: string): string {
  const value = payload[name] ?? fallback
  if (typeof value !== 'string' || !DECIMAL.test(value)) {
    throw new Error(`[invalid_argument] ${name} must be a non-negative decimal integer string`)
  }
  return value
}

function integer(
  payload: Record<string, unknown>,
  name: string,
  fallback: number,
  maximum: number,
): number {
  const value = payload[name] ?? fallback
  if (!Number.isSafeInteger(value) || (value as number) < 1 || (value as number) > maximum) {
    throw new Error(`[invalid_argument] ${name} must be an integer from 1 through ${String(maximum)}`)
  }
  return value as number
}

function taskStatus(payload: Record<string, unknown>): NativeTask['status'] {
  const value = payload.status
  if (typeof value !== 'string' || !STATUS.has(value as NativeTask['status'])) {
    throw new Error('[invalid_argument] status must be todo, in_progress, in_review, or done')
  }
  return value as NativeTask['status']
}

async function dispatchRemote(
  api: CollabRemoteApi,
  actorId: string,
  endpoint: string,
  payload: unknown,
): Promise<unknown> {
  const input = record(payload)
  switch (endpoint) {
    case 'snapshot':
      return await api.snapshot(actorId)
    case 'actors':
      return await api.listActors(actorId)
    case 'target.members':
      return await api.listTargetMembers(actorId, requiredString(input, 'targetId'))
    case 'changes':
      return await api.listChanges(
        actorId,
        decimalString(input, 'afterSeq', '0'),
        integer(input, 'limit', 100, 500),
      )
    case 'history':
      return await api.readMessages(
        actorId,
        requiredString(input, 'targetId'),
        decimalString(input, 'afterSeq', '0'),
        integer(input, 'limit', 50, 100),
      )
    case 'history.tail':
      return await api.readMessagesTail(
        actorId,
        requiredString(input, 'targetId'),
        integer(input, 'limit', 10, 100),
      )
    case 'inbox.list': {
      const filter = optionalString(input, 'filter')
      if (filter !== undefined && filter !== 'all' && filter !== 'unread') {
        throw new Error(`[invalid_argument] filter must be 'unread' or 'all', got '${filter}'`)
      }
      return await api.inboxList(
        actorId,
        integer(input, 'limit', 20, 50),
        optionalString(input, 'cursor'),
        filter as 'all' | 'unread' | undefined,
      )
    }
    case 'inbox.doneAll':
      return await api.inboxDoneAll(actorId)
    case 'inbox.done':
      await api.inboxDone(
        actorId,
        requiredString(input, 'targetId'),
        decimalString(input, 'throughSeq'),
      )
      return null
    case 'tasks':
      return await api.listTasks(actorId, optionalString(input, 'targetId'))
    case 'agent.presets':
      return await api.listAgentPresets()
    case 'agent.profile':
      return await api.agentProfile(actorId, requiredString(input, 'agentId'))
    case 'agent.profiles':
      return await api.agentProfiles(actorId)
    case 'agent.profile.update':
      return await api.updateAgentProfile(
        actorId,
        requiredString(input, 'agentId'),
        requiredString(input, 'displayName'),
        requiredString(input, 'description'),
        decimalString(input, 'expectedProfileVersion'),
      )
    case 'agent.runtime.replace':
      return await api.replaceAgentRuntime(
        actorId,
        requiredString(input, 'agentId'),
        requiredString(input, 'provider'),
        requiredString(input, 'model'),
        requiredString(input, 'presetId'),
        optionalString(input, 'expectedGeneration'),
      )
    case 'agent.memberships':
      return await api.agentMemberships(actorId, requiredString(input, 'agentId'))
    case 'agent.workspace.list':
      return await api.agentWorkspace(
        actorId,
        requiredString(input, 'agentId'),
        pathString(input, 'dirPath'),
        booleanValue(input, 'includeHidden', false),
      )
    case 'agent.workspace.read':
      return await api.agentWorkspaceFile(
        actorId,
        requiredString(input, 'agentId'),
        requiredString(input, 'path'),
      )
    case 'agent.create':
      return await api.createConfiguredAgent(
        requiredString(input, 'displayName'),
        requiredString(input, 'handle'),
        requiredString(input, 'description'),
        requiredString(input, 'provider'),
        requiredString(input, 'model'),
        requiredString(input, 'presetId'),
      )
    case 'agent.delete':
      await api.deleteAgent(requiredString(input, 'agentId'))
      return null
    case 'runtime.bindings':
      return await api.listRuntimeBindings()
    case 'channel.create':
      return await api.createChannel(requiredString(input, 'name'), actorId)
    case 'direct.create':
      return await api.createDirect(actorId, requiredString(input, 'peerId'))
    case 'thread.create':
      return await api.createThread(requiredString(input, 'rootMessageId'), actorId)
    case 'thread.summaries':
      return await api.threadSummaries(
        actorId,
        stringArray(input, 'rootMessageIds', 100),
      )
    case 'thread.follow':
      await api.followThread(requiredString(input, 'threadTargetId'), actorId)
      return null
    case 'thread.unfollow':
      await api.unfollowThread(requiredString(input, 'threadTargetId'), actorId)
      return null
    case 'member.add':
      await api.addMember(
        requiredString(input, 'targetId'),
        requiredString(input, 'memberId'),
        actorId,
      )
      return null
    case 'message.send':
      return await api.sendMessage({
        targetId: requiredString(input, 'targetId'),
        authorId: actorId,
        clientRequestId: requiredString(input, 'requestId'),
        text: requiredString(input, 'text'),
      })
    case 'task.create':
      return await api.createTask(requiredString(input, 'messageId'), actorId)
    case 'task.claim':
      return await api.claimTask(
        requiredString(input, 'messageId'),
        optionalString(input, 'actorId') ?? actorId,
      )
    case 'task.unclaim':
      return await api.unclaimTask(
        requiredString(input, 'messageId'),
        actorId,
        decimalString(input, 'expectedVersion'),
      )
    case 'task.update':
      return await api.updateTaskStatus(
        requiredString(input, 'messageId'),
        actorId,
        taskStatus(input),
        decimalString(input, 'expectedVersion'),
      )
    default:
      throw new Error(`[not_found] unknown dsh-chaos endpoint '${endpoint}'`)
  }
}

/** Build the fixed-principal RPC handler used by DSH's trusted Connection carrier. */
export function createCollabRpcHandler(
  api: CollabRemoteApi,
  actorId: string,
): ConnectionRpcHandler {
  return async (endpoint, payload, signal) => {
    if (signal.aborted) return { ok: false, error: { code: 'cancelled', message: 'cancelled', details: {} } }
    try {
      return { ok: true, value: success(await dispatchRemote(api, actorId, endpoint, payload)) }
    } catch (error) {
      return { ok: true, value: errorResult(error) }
    }
  }
}

function isLoopbackHostname(hostname: string): boolean {
  if (hostname === 'localhost' || hostname === '[::1]') return true
  const parts = hostname.split('.')
  return parts.length === 4
    && parts[0] === '127'
    && parts.every(part => /^\d{1,3}$/.test(part) && Number(part) <= 255)
}

/** Apply the same loopback/same-origin posture as a loopback Connection RPC channel. */
export function isTrustedSseRequest(request: IncomingMessage): boolean {
  const host = request.headers.host
  if (typeof host !== 'string') return false
  let hostUrl: URL
  try {
    hostUrl = new URL(`http://${host}`)
  } catch {
    return false
  }
  if (!isLoopbackHostname(hostUrl.hostname)) return false
  if (request.headers['sec-fetch-site'] === 'cross-site') return false
  const origin = request.headers.origin
  if (origin === undefined) return true
  if (typeof origin !== 'string') return false
  try {
    return new URL(origin).host === hostUrl.host
  } catch {
    return false
  }
}

function requestCursor(request: IncomingMessage): string {
  const lastEventId = request.headers['last-event-id']
  const headerCursor = typeof lastEventId === 'string' && lastEventId !== ''
    ? lastEventId
    : undefined
  const queryCursor = new URL(request.url ?? COLLAB_EVENTS_PATH, 'http://localhost')
    .searchParams.get('cursor') ?? undefined
  const cursor = headerCursor ?? queryCursor ?? '0'
  if (!DECIMAL.test(cursor)) throw new Error('[invalid_argument] SSE cursor must be a decimal integer')
  return cursor
}

interface PreparedWait {
  promise: Promise<'change' | 'heartbeat' | 'aborted'>
  cancel(): void
}

function prepareWait(api: CollabRemoteApi, signal: AbortSignal, heartbeatMs: number): PreparedWait {
  let settled = false
  let resolvePromise!: (reason: 'change' | 'heartbeat' | 'aborted') => void
  let unsubscribe = (): void => {}
  let timer: ReturnType<typeof setTimeout> | undefined
  const promise = new Promise<'change' | 'heartbeat' | 'aborted'>((resolve) => {
    resolvePromise = resolve
  })
  const finish = (reason: 'change' | 'heartbeat' | 'aborted'): void => {
    if (settled) return
    settled = true
    if (timer !== undefined) clearTimeout(timer)
    unsubscribe()
    signal.removeEventListener('abort', onAbort)
    resolvePromise(reason)
  }
  const onAbort = (): void => { finish('aborted') }
  const registered = api.onChange(() => { finish('change') })
  if (settled) registered()
  else unsubscribe = registered
  if (!settled) timer = setTimeout(() => { finish('heartbeat') }, heartbeatMs)
  signal.addEventListener('abort', onAbort, { once: true })
  if (signal.aborted) finish('aborted')
  return {
    promise,
    cancel: () => { finish('aborted') },
  }
}

async function writeSse(response: ServerResponse, signal: AbortSignal, chunk: string): Promise<void> {
  if (signal.aborted || response.destroyed) return
  if (response.write(chunk)) return
  await new Promise<void>((resolve) => {
    const finish = (): void => {
      response.off('drain', finish)
      response.off('close', finish)
      signal.removeEventListener('abort', finish)
      resolve()
    }
    response.once('drain', finish)
    response.once('close', finish)
    signal.addEventListener('abort', finish, { once: true })
  })
}

/** Serve recipient-filtered durable changes with standard Last-Event-ID replay. */
export async function serveCollabEvents(
  request: IncomingMessage,
  response: ServerResponse,
  api: CollabRemoteApi,
  actorId: string,
  config: CollabRemoteConfig,
): Promise<void> {
  if (request.method !== 'GET') {
    response.writeHead(405, { allow: 'GET' })
    response.end('method not allowed')
    return
  }
  if (!isTrustedSseRequest(request)) {
    response.writeHead(403)
    response.end('forbidden')
    return
  }

  let cursor: string
  try {
    cursor = requestCursor(request)
  } catch (error) {
    response.writeHead(400)
    response.end(error instanceof Error ? error.message : String(error))
    return
  }

  response.writeHead(200, {
    'content-type': 'text/event-stream; charset=utf-8',
    'cache-control': 'no-cache, no-transform',
    connection: 'keep-alive',
    'x-accel-buffering': 'no',
  })
  response.flushHeaders()
  const controller = new AbortController()
  const abort = (): void => { controller.abort() }
  response.once('close', abort)
  try {
    await writeSse(response, controller.signal, 'retry: 1000\n: connected\n\n')
    while (!controller.signal.aborted) {
      const waiter = prepareWait(api, controller.signal, config.heartbeatMs)
      let changes: NativeChangeEvent[]
      try {
        changes = await api.listChanges(actorId, cursor, 500)
      } catch (error) {
        waiter.cancel()
        const detail = errorResult(error).error
        const event = detail.code === 'change_cursor_resync_required'
          ? 'resync_required'
          : 'error'
        await writeSse(
          response,
          controller.signal,
          `event: ${event}\ndata: ${JSON.stringify(detail)}\n\n`,
        )
        return
      }
      if (changes.length > 0) {
        waiter.cancel()
        for (const change of changes) {
          cursor = change.seq
          await writeSse(
            response,
            controller.signal,
            `id: ${change.seq}\nevent: change\ndata: ${JSON.stringify(change)}\n\n`,
          )
        }
        continue
      }
      const reason = await waiter.promise
      if (reason === 'heartbeat') {
        await writeSse(response, controller.signal, ': heartbeat\n\n')
      }
    }
  } finally {
    response.off('close', abort)
    if (!response.writableEnded) response.end()
  }
}

/** Register the host RPC channel and exact SSE route when Web services exist. */
export function installCollabRemote(
  ctx: Context,
  api: CollabRemoteApi,
  actorId: string,
  config: CollabRemoteConfig,
): void {
  const handler = createCollabRpcHandler(api, actorId)
  ctx.effect(
    () => ctx.connection.rpc.handle(COLLAB_RPC_CHANNEL, handler, { authority: 'loopback' }),
    'dsh-chaos: remote RPC',
  )
  ctx.effect(
    () => ctx.webServer.register({
      kind: 'exact',
      path: COLLAB_EVENTS_PATH,
      handler: (request, response) => serveCollabEvents(request, response, api, actorId, config),
    }),
    'dsh-chaos: SSE changes',
  )
}
