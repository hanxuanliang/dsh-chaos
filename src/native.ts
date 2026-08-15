import { createRequire } from 'node:module'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export interface NativeActor {
  id: string
  kind: 'user' | 'agent'
  handle: string
  displayName: string
  createdAtMs: number
}

export interface NativeTarget {
  id: string
  kind: 'channel' | 'direct' | 'thread'
  name: string
  createdBy: string
  createdAtMs: number
}

export interface NativeMessage {
  seq: string
  id: string
  targetId: string
  authorId: string
  clientRequestId: string
  text: string
  createdAtMs: number
}

export interface NativeSendResult {
  message: NativeMessage
  recipientIds: string[]
  wakeAgentIds: string[]
  replayed: boolean
}

export interface NativeRuntimeBinding {
  agentId: string
  sessionId: string
  generation: string
  provider: string
  model: string
  preset: string
  boundAtMs: number
}

export interface NativeInboxBatch {
  id?: string
  agentId: string
  sessionId: string
  generation: string
  messages: Array<{ deliveryId: string; message: NativeMessage }>
  checkedAtMs: number
}

export interface NativeTask {
  messageId: string
  targetId: string
  number: string
  status: 'todo' | 'in_progress' | 'in_review' | 'done'
  assigneeId?: string
  version: string
  createdAtMs: number
  updatedAtMs: number
}

export interface NativeCollabHandle {
  close(): Promise<void>
  createUser(handle: string, displayName: string): Promise<NativeActor>
  createAgent(handle: string, displayName: string, workspacePath: string): Promise<NativeActor>
  createChannel(name: string, creatorId: string): Promise<NativeTarget>
  addMember(targetId: string, actorId: string, addedBy: string): Promise<void>
  sendMessage(input: {
    targetId: string
    authorId: string
    clientRequestId: string
    text: string
  }): Promise<NativeSendResult>
  bindRuntime(
    agentId: string,
    sessionId: string,
    provider: string,
    model: string,
    preset: string,
  ): Promise<NativeRuntimeBinding>
  checkInbox(agentId: string, generation: string, sessionId: string, limit: number): Promise<NativeInboxBatch>
  markModelSeen(batchId: string, agentId: string, generation: string, sessionId: string): Promise<void>
  createTask(messageId: string, actorId: string): Promise<NativeTask>
  claimTask(messageId: string, actorId: string): Promise<NativeTask>
}

interface NativeModule {
  openCollab(path: string): Promise<NativeCollabHandle>
}

let loaded: NativeModule | undefined

export function loadNativeModule(): NativeModule {
  if (loaded !== undefined) return loaded
  const require = createRequire(import.meta.url)
  const packageRoot = resolve(fileURLToPath(new URL('.', import.meta.url)), '..')
  const nativePath = process.env.DSH_CHAOS_NATIVE_PATH
    ?? resolve(packageRoot, 'native', 'dsh_chaos_core.node')
  loaded = require(nativePath) as NativeModule
  return loaded
}
