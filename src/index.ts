/**
 * DSH host adapter for the Rust/Turso chaos collaboration core.
 * @module dsh-chaos
 */

import { mkdir } from 'node:fs/promises'
import { homedir, userInfo } from 'node:os'
import { join, resolve } from 'node:path'
import { randomBytes } from 'node:crypto'
import { Context, Service } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'
import type { StreamChunk } from '@deepseek-ai/dsh-llm'
import { DeliveryBridge } from './delivery.ts'
import { installCollabRemote } from './remote.ts'
import { RuntimeManager, type CreateRuntimeInput } from './runtime.ts'
import { loadNativeModule, type NativeCollabHandle, type NativeRuntimeBinding } from './native.ts'

export { DeliveryBridge } from './delivery.ts'
export { RuntimeManager } from './runtime.ts'
export type { CreateRuntimeInput } from './runtime.ts'
export { installCollabTools } from './tools.ts'
export {
  COLLAB_EVENTS_PATH,
  COLLAB_RPC_CHANNEL,
  createCollabRpcHandler,
  installCollabRemote,
  isTrustedSseRequest,
  serveCollabEvents,
} from './remote.ts'
export type {
  CollabDomainError,
  CollabDomainResult,
  CollabRemoteApi,
  CollabRemoteConfig,
} from './remote.ts'

export type {
  NativeActor as Actor,
  NativeChangeEvent as ChangeEvent,
  NativeCollabSnapshot as CollabSnapshot,
  NativeInboxBatch as InboxBatch,
  NativeMessage as Message,
  NativePendingWake as PendingWake,
  NativeRuntimeBinding as RuntimeBinding,
  NativeSendResult as SendMessageResult,
  NativeTarget as Target,
  NativeTask as Task,
} from './native.ts'

export const name = 'dsh-chaos'

const CHANGE_RETENTION_MS = 7 * 24 * 60 * 60 * 1_000
const CHANGE_PRUNE_INTERVAL_MS = 60 * 60 * 1_000

export interface Config {
  path?: string
  deliveryPollMs?: number
  remoteEnabled?: boolean
  webUserHandle?: string
  webUserDisplayName?: string
  sseHeartbeatMs?: number
}

function dshHome(): string {
  return resolve(process.env.DSH_HOME?.trim() || join(homedir(), '.dsh'))
}

const DEFAULT_DATABASE_PATH = join(dshHome(), 'collab', 'state.db')

function slugifyHandle(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
  return slug === '' ? 'agent' : slug.slice(0, 40)
}

function defaultWebUserDisplayName(): string {
  try {
    const username = userInfo().username.trim()
    return username === '' ? 'Local User' : username
  } catch {
    return 'Local User'
  }
}

export const Config: z<Config> = z.object({
  path: z.string().default(DEFAULT_DATABASE_PATH),
  deliveryPollMs: z.number().step(1).min(50).default(500),
  remoteEnabled: z.boolean().default(true),
  webUserHandle: z.string().default('local-user'),
  webUserDisplayName: z.string().default(''),
  sseHeartbeatMs: z.number().step(1).min(1_000).default(15_000),
})

declare module '@deepseek-ai/cordis' {
  interface Context {
    collab: CollabService
  }
}

/** Host-side stable collab service. DSH runtime wiring consumes this service;
 * only this class may call the native handle. */
export class CollabService extends Service {
  static inject = ['agentLoop', 'agents', 'tools', 'llm']
  static Config: z<Config> = Config

  private handle: NativeCollabHandle | undefined
  private runtimes: RuntimeManager | undefined
  private delivery: DeliveryBridge | undefined
  private readonly changeListeners = new Set<() => void>()

  constructor(ctx: Context, private readonly config: Config = {}) {
    super(ctx, 'collab')
  }

  protected async [Service.init](): Promise<void> {
    const path = this.config.path ?? DEFAULT_DATABASE_PATH
    const handle = await loadNativeModule().openCollab(path)
    this.handle = handle
    let retentionStopped = false
    let retentionTimer: ReturnType<typeof setInterval> | undefined
    let retentionRun = Promise.resolve()
    this.ctx.effect(() => async () => {
      const errors: unknown[] = []
      retentionStopped = true
      if (retentionTimer !== undefined) clearInterval(retentionTimer)
      await retentionRun
      const delivery = this.delivery
      this.delivery = undefined
      try {
        await delivery?.stop()
      } catch (error) {
        errors.push(error)
      }
      const runtimes = this.runtimes
      this.runtimes = undefined
      try {
        await runtimes?.close()
      } catch (error) {
        errors.push(error)
      }
      this.changeListeners.clear()
      if (this.handle === handle) this.handle = undefined
      try {
        await handle.close()
      } catch (error) {
        errors.push(error)
      }
      if (errors.length > 0) throw new AggregateError(errors, 'dsh-chaos runtime stack cleanup failed')
    }, 'dsh-chaos.closeRuntimeStack')

    const warnings = {
      warn: (message: string, error?: unknown) => {
        this.ctx.logger.warn(message)
        if (error !== undefined) this.ctx.logger.warn(error)
      },
    }
    await handle.pruneChangesBefore(Date.now() - CHANGE_RETENTION_MS)
    const pruneExpiredChanges = (): void => {
      retentionRun = retentionRun.then(async () => {
        if (retentionStopped) return
        await handle.pruneChangesBefore(Date.now() - CHANGE_RETENTION_MS)
      }).catch((error: unknown) => {
        warnings.warn('dsh-chaos change retention pruning failed', error)
      })
    }
    retentionTimer = setInterval(pruneExpiredChanges, CHANGE_PRUNE_INTERVAL_MS)
    retentionTimer.unref()
    const runtimes = new RuntimeManager(this.ctx.agents, this, warnings)
    this.runtimes = runtimes

    if (this.config.remoteEnabled ?? true) {
      // Default to the OS username; the stable handle keeps identity while
      // ensure_user migrates the display name of legacy 'Local User' rows.
      const webActor = await handle.ensureUser(
        this.config.webUserHandle ?? 'local-user',
        this.config.webUserDisplayName || defaultWebUserDisplayName(),
      )
      this.ctx.inject(['connection', 'webServer'], (remoteCtx) => {
        installCollabRemote(remoteCtx, this, webActor.id, {
          heartbeatMs: this.config.sseHeartbeatMs ?? 15_000,
        })
      })
    }

    this.ctx.on('llm/stream', (options, next) => {
      const stream = next()
      return options.sessionId === undefined
        ? stream
        : confirmBeforeIteration(runtimes, String(options.sessionId), stream)
    }, { global: true, prepend: true })

    await runtimes.restore()
    const delivery = new DeliveryBridge(
      this,
      runtimes,
      warnings,
      this.config.deliveryPollMs ?? 500,
    )
    this.delivery = delivery
    delivery.start()
  }

  async createUser(handle: string, displayName: string) {
    const actor = await this.requireHandle().createUser(handle, displayName)
    this.publishChange()
    return actor
  }

  async ensureUser(handle: string, displayName: string) {
    const actor = await this.requireHandle().ensureUser(handle, displayName)
    this.publishChange()
    return actor
  }

  async createAgent(handle: string, displayName: string, workspacePath: string) {
    const actor = await this.requireHandle().createAgent(handle, displayName, workspacePath)
    this.publishChange()
    return actor
  }

  /** Human types a name; home dir, Session, and binding stay off-screen. */
  async createNamedAgent(name: string) {
    const displayName = name.trim()
    if (displayName === '') throw new Error('[invalid_argument] name must not be blank')
    const base = slugifyHandle(displayName)
    const template = join(dshHome(), 'agents', '{id}')
    let actor
    try {
      actor = await this.requireHandle().createAgent(base, displayName, template)
    } catch (error) {
      const suffix = randomBytes(2).toString('hex')
      actor = await this.requireHandle().createAgent(`${base}-${suffix}`, displayName, template)
      void error
    }
    const workspacePath = join(dshHome(), 'agents', actor.id)
    await mkdir(workspacePath, { recursive: true })
    let binding: NativeRuntimeBinding | undefined
    try {
      binding = await this.createRuntime({
        agentId: actor.id,
        workspacePath,
        provider: 'default',
        model: 'default',
        preset: 'default',
      })
    } catch (error) {
      this.ctx.logger.warn(`dsh-chaos: created Agent ${actor.id} without a Session`)
      this.ctx.logger.warn(error)
    }
    this.publishChange()
    return binding === undefined
      ? { actor, workspacePath }
      : { actor, binding, workspacePath }
  }

  async createChannel(name: string, creatorId: string) {
    const target = await this.requireHandle().createChannel(name, creatorId)
    this.publishChange()
    return target
  }

  async createDirect(actorId: string, peerId: string) {
    const target = await this.requireHandle().createDirect(actorId, peerId)
    this.publishChange()
    return target
  }

  async createThread(rootMessageId: string, actorId: string) {
    const target = await this.requireHandle().createThread(rootMessageId, actorId)
    this.publishChange()
    return target
  }

  async followThread(threadTargetId: string, actorId: string) {
    await this.requireHandle().followThread(threadTargetId, actorId)
    this.publishChange()
  }

  async unfollowThread(threadTargetId: string, actorId: string) {
    await this.requireHandle().unfollowThread(threadTargetId, actorId)
    this.publishChange()
  }

  async addMember(targetId: string, actorId: string, addedBy: string) {
    await this.requireHandle().addMember(targetId, actorId, addedBy)
    this.publishChange()
  }

  async sendMessage(input: Parameters<NativeCollabHandle['sendMessage']>[0]) {
    const result = await this.requireHandle().sendMessage(input)
    this.delivery?.kick()
    this.publishChange()
    return result
  }

  bindRuntime(
    agentId: string,
    sessionId: string,
    provider: string,
    model: string,
    preset: string,
  ) {
    return this.requireHandle().bindRuntime(agentId, sessionId, provider, model, preset)
  }

  runtimeBinding(agentId: string) {
    return this.requireHandle().runtimeBinding(agentId)
  }

  runtimeBindingForSession(sessionId: string) {
    return this.requireHandle().runtimeBindingForSession(sessionId)
  }

  listRuntimeBindings() {
    return this.requireHandle().listRuntimeBindings()
  }

  listPendingWakes(limit = 1000) {
    return this.requireHandle().listPendingWakes(limit)
  }

  markNotified(agentId: string, generation: string, sessionId: string, pendingSeq: string) {
    return this.requireHandle().markNotified(agentId, generation, sessionId, pendingSeq)
  }

  rearmRuntimeWake(agentId: string, generation: string, sessionId: string) {
    return this.requireHandle().rearmRuntimeWake(agentId, generation, sessionId)
  }

  checkInbox(agentId: string, generation: string, sessionId: string, limit = 50) {
    return this.requireHandle().checkInbox(agentId, generation, sessionId, limit)
  }

  markModelSeen(batchId: string, agentId: string, generation: string, sessionId: string) {
    return this.requireHandle().markModelSeen(batchId, agentId, generation, sessionId)
  }

  readMessage(actorId: string, targetId: string, messageId: string) {
    return this.requireHandle().readMessage(actorId, targetId, messageId)
  }

  readMessages(actorId: string, targetId: string, afterSeq = '0', limit = 50) {
    return this.requireHandle().readMessages(actorId, targetId, afterSeq, limit)
  }

  readMessagesTail(actorId: string, targetId: string, limit = 10) {
    return this.requireHandle().readMessagesTail(actorId, targetId, limit)
  }

  listActors(actorId: string) {
    return this.requireHandle().listActors(actorId)
  }

  listTargetMembers(actorId: string, targetId: string) {
    return this.requireHandle().listTargetMembers(actorId, targetId)
  }

  snapshot(actorId: string) {
    return this.requireHandle().snapshot(actorId)
  }

  listChanges(actorId: string, afterSeq = '0', limit = 100) {
    return this.requireHandle().listChanges(actorId, afterSeq, limit)
  }

  pruneChangesBefore(beforeMs: number) {
    return this.requireHandle().pruneChangesBefore(beforeMs)
  }

  createRuntime(input: CreateRuntimeInput) {
    return this.requireRuntimes().create(input)
  }

  resetRuntime(input: CreateRuntimeInput) {
    return this.requireRuntimes().reset(input)
  }

  resumeRuntime(agentId: string) {
    return this.requireRuntimes().resume(agentId)
  }

  stopRuntime(agentId: string) {
    return this.requireRuntimes().stop(agentId)
  }

  async createTask(messageId: string, actorId: string) {
    const task = await this.requireHandle().createTask(messageId, actorId)
    this.publishChange()
    return task
  }

  async claimTask(messageId: string, actorId: string) {
    const task = await this.requireHandle().claimTask(messageId, actorId)
    this.publishChange()
    return task
  }

  listTasks(actorId: string, targetId?: string) {
    return this.requireHandle().listTasks(actorId, targetId)
  }

  async unclaimTask(messageId: string, actorId: string, expectedVersion: string) {
    const task = await this.requireHandle().unclaimTask(messageId, actorId, expectedVersion)
    this.publishChange()
    return task
  }

  async updateTaskStatus(
    messageId: string,
    actorId: string,
    status: Parameters<NativeCollabHandle['updateTaskStatus']>[2],
    expectedVersion: string,
  ) {
    const task = await this.requireHandle().updateTaskStatus(
      messageId,
      actorId,
      status,
      expectedVersion,
    )
    this.publishChange()
    return task
  }

  onChange(listener: () => void): () => void {
    this.changeListeners.add(listener)
    return () => { this.changeListeners.delete(listener) }
  }

  private requireHandle(): NativeCollabHandle {
    if (this.handle === undefined) throw new Error('dsh-chaos collab service is not active')
    return this.handle
  }

  private requireRuntimes(): RuntimeManager {
    if (this.runtimes === undefined) throw new Error('dsh-chaos RuntimeManager is not active')
    return this.runtimes
  }

  private publishChange(): void {
    for (const listener of [...this.changeListeners]) {
      try {
        listener()
      } catch (error) {
        this.ctx.logger.warn('dsh-chaos change listener threw')
        this.ctx.logger.warn(error)
      }
    }
  }
}

async function* confirmBeforeIteration(
  runtimes: RuntimeManager,
  sessionId: string,
  stream: AsyncIterable<StreamChunk>,
): AsyncIterable<StreamChunk> {
  await runtimes.confirmModelSeen(sessionId)
  yield* stream
}

export default CollabService
