/**
 * DSH host adapter for the Rust/Turso chaos collaboration core.
 * @module dsh-chaos
 */

import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import { Context, Service } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'
import type { StreamChunk } from '@deepseek-ai/dsh-llm'
import { DeliveryBridge } from './delivery.ts'
import { RuntimeManager, type CreateRuntimeInput } from './runtime.ts'
import { loadNativeModule, type NativeCollabHandle } from './native.ts'

export { DeliveryBridge } from './delivery.ts'
export { RuntimeManager } from './runtime.ts'
export type { CreateRuntimeInput } from './runtime.ts'
export { installCollabTools } from './tools.ts'

export type {
  NativeActor as Actor,
  NativeInboxBatch as InboxBatch,
  NativeMessage as Message,
  NativePendingWake as PendingWake,
  NativeRuntimeBinding as RuntimeBinding,
  NativeSendResult as SendMessageResult,
  NativeTarget as Target,
  NativeTask as Task,
} from './native.ts'

export const name = 'dsh-chaos'

export interface Config {
  path?: string
  deliveryPollMs?: number
}

const DEFAULT_DATABASE_PATH = join(
  resolve(process.env.DSH_HOME?.trim() || join(homedir(), '.dsh')),
  'collab',
  'state.db',
)

export const Config: z<Config> = z.object({
  path: z.string().default(DEFAULT_DATABASE_PATH),
  deliveryPollMs: z.number().step(1).min(50).default(500),
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

  constructor(ctx: Context, private readonly config: Config = {}) {
    super(ctx, 'collab')
  }

  protected async [Service.init](): Promise<void> {
    const path = this.config.path ?? DEFAULT_DATABASE_PATH
    const handle = await loadNativeModule().openCollab(path)
    this.handle = handle
    this.ctx.effect(() => async () => {
      const errors: unknown[] = []
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
    const runtimes = new RuntimeManager(this.ctx.agents, this, warnings)
    this.runtimes = runtimes

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

  createUser(handle: string, displayName: string) {
    return this.requireHandle().createUser(handle, displayName)
  }

  createAgent(handle: string, displayName: string, workspacePath: string) {
    return this.requireHandle().createAgent(handle, displayName, workspacePath)
  }

  createChannel(name: string, creatorId: string) {
    return this.requireHandle().createChannel(name, creatorId)
  }

  createDirect(actorId: string, peerId: string) {
    return this.requireHandle().createDirect(actorId, peerId)
  }

  createThread(rootMessageId: string, actorId: string) {
    return this.requireHandle().createThread(rootMessageId, actorId)
  }

  followThread(threadTargetId: string, actorId: string) {
    return this.requireHandle().followThread(threadTargetId, actorId)
  }

  unfollowThread(threadTargetId: string, actorId: string) {
    return this.requireHandle().unfollowThread(threadTargetId, actorId)
  }

  addMember(targetId: string, actorId: string, addedBy: string) {
    return this.requireHandle().addMember(targetId, actorId, addedBy)
  }

  async sendMessage(input: Parameters<NativeCollabHandle['sendMessage']>[0]) {
    const result = await this.requireHandle().sendMessage(input)
    this.delivery?.kick()
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

  createTask(messageId: string, actorId: string) {
    return this.requireHandle().createTask(messageId, actorId)
  }

  claimTask(messageId: string, actorId: string) {
    return this.requireHandle().claimTask(messageId, actorId)
  }

  private requireHandle(): NativeCollabHandle {
    if (this.handle === undefined) throw new Error('dsh-chaos collab service is not active')
    return this.handle
  }

  private requireRuntimes(): RuntimeManager {
    if (this.runtimes === undefined) throw new Error('dsh-chaos RuntimeManager is not active')
    return this.runtimes
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
