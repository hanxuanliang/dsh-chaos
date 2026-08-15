/**
 * DSH host adapter for the Rust/Turso chaos collaboration core.
 * @module dsh-chaos
 */

import { homedir } from 'node:os'
import { join, resolve } from 'node:path'
import { Context, Service } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'
import { loadNativeModule, type NativeCollabHandle } from './native.ts'

export type {
  NativeActor as Actor,
  NativeInboxBatch as InboxBatch,
  NativeMessage as Message,
  NativeRuntimeBinding as RuntimeBinding,
  NativeSendResult as SendMessageResult,
  NativeTarget as Target,
  NativeTask as Task,
} from './native.ts'

export const name = 'dsh-chaos'

export interface Config {
  path?: string
}

const DEFAULT_DATABASE_PATH = join(
  resolve(process.env.DSH_HOME?.trim() || join(homedir(), '.dsh')),
  'collab',
  'state.db',
)

export const Config: z<Config> = z.object({
  path: z.string().default(DEFAULT_DATABASE_PATH),
})

declare module '@deepseek-ai/cordis' {
  interface Context {
    collab: CollabService
  }
}

/** Host-side stable collab service. DSH runtime wiring consumes this service;
 * only this class may call the native handle. */
export class CollabService extends Service {
  static Config: z<Config> = Config

  private handle: NativeCollabHandle | undefined

  constructor(ctx: Context, private readonly config: Config = {}) {
    super(ctx, 'collab')
  }

  protected async [Service.init](): Promise<void> {
    const path = this.config.path ?? DEFAULT_DATABASE_PATH
    const handle = await loadNativeModule().openCollab(path)
    this.handle = handle
    this.ctx.effect(() => async () => {
      if (this.handle === handle) this.handle = undefined
      await handle.close()
    }, 'dsh-chaos.closeNativeCore')
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

  addMember(targetId: string, actorId: string, addedBy: string) {
    return this.requireHandle().addMember(targetId, actorId, addedBy)
  }

  sendMessage(input: Parameters<NativeCollabHandle['sendMessage']>[0]) {
    return this.requireHandle().sendMessage(input)
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

  checkInbox(agentId: string, generation: string, sessionId: string, limit = 50) {
    return this.requireHandle().checkInbox(agentId, generation, sessionId, limit)
  }

  markModelSeen(batchId: string, agentId: string, generation: string, sessionId: string) {
    return this.requireHandle().markModelSeen(batchId, agentId, generation, sessionId)
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
}

export default CollabService
