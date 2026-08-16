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
  parentTargetId?: string
  rootMessageId?: string
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

export interface NativePendingWake {
  binding: NativeRuntimeBinding
  pendingSeq: string
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

export interface NativeChangeEvent {
  seq: string
  kind:
    | 'actor_created'
    | 'target_created'
    | 'membership_changed'
    | 'thread_follow_changed'
    | 'message_created'
    | 'task_created'
    | 'task_updated'
  targetId?: string
  entityId: string
  createdAtMs: number
}

export interface NativeCollabSnapshot {
  actor: NativeActor
  cursor: string
  targets: NativeTarget[]
  followedThreadIds: string[]
  tasks: NativeTask[]
}

/**
 * Authoritative tail page of one target. `count` is the exact total message
 * count (decimal string), never a lower bound; `messages` is the true latest
 * page in ascending order.
 */
export interface NativeMessageTail {
  count: string
  messages: NativeMessage[]
}

export interface NativeCollabHandle {
  close(): Promise<void>
  createUser(handle: string, displayName: string): Promise<NativeActor>
  ensureUser(handle: string, displayName: string): Promise<NativeActor>
  createAgent(handle: string, displayName: string, workspacePath: string): Promise<NativeActor>
  createChannel(name: string, creatorId: string): Promise<NativeTarget>
  createDirect(actorId: string, peerId: string): Promise<NativeTarget>
  createThread(rootMessageId: string, actorId: string): Promise<NativeTarget>
  followThread(threadTargetId: string, actorId: string): Promise<void>
  unfollowThread(threadTargetId: string, actorId: string): Promise<void>
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
  runtimeBinding(agentId: string): Promise<NativeRuntimeBinding | undefined>
  runtimeBindingForSession(sessionId: string): Promise<NativeRuntimeBinding | undefined>
  listRuntimeBindings(): Promise<NativeRuntimeBinding[]>
  listPendingWakes(limit: number): Promise<NativePendingWake[]>
  markNotified(
    agentId: string,
    generation: string,
    sessionId: string,
    pendingSeq: string,
  ): Promise<void>
  rearmRuntimeWake(agentId: string, generation: string, sessionId: string): Promise<void>
  checkInbox(agentId: string, generation: string, sessionId: string, limit: number): Promise<NativeInboxBatch>
  markModelSeen(batchId: string, agentId: string, generation: string, sessionId: string): Promise<void>
  readMessage(actorId: string, targetId: string, messageId: string): Promise<NativeMessage>
  readMessages(actorId: string, targetId: string, afterSeq: string, limit: number): Promise<NativeMessage[]>
  readMessagesTail(actorId: string, targetId: string, limit: number): Promise<NativeMessageTail>
  listActors(actorId: string): Promise<NativeActor[]>
  snapshot(actorId: string): Promise<NativeCollabSnapshot>
  listChanges(actorId: string, afterSeq: string, limit: number): Promise<NativeChangeEvent[]>
  pruneChangesBefore(beforeMs: number): Promise<string>
  createTask(messageId: string, actorId: string): Promise<NativeTask>
  claimTask(messageId: string, actorId: string): Promise<NativeTask>
  listTasks(actorId: string, targetId?: string): Promise<NativeTask[]>
  unclaimTask(messageId: string, actorId: string, expectedVersion: string): Promise<NativeTask>
  updateTaskStatus(
    messageId: string,
    actorId: string,
    status: NativeTask['status'],
    expectedVersion: string,
  ): Promise<NativeTask>
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
