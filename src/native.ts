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

export interface NativeAgentCharter {
  schemaVersion: number
  summary: string
  capabilities: string[]
  constraints: string[]
}

export interface NativeAgentProfile {
  actor: NativeActor
  workspacePath: string
  lifecycle: 'active' | 'archived'
  charter: NativeAgentCharter
  version: string
  createdAtMs: number
  updatedAtMs: number
}

export interface NativeTargetMember {
  actor: NativeActor
  role: 'owner' | 'member'
  joinedAtMs: number
}

export interface NativeAgentMembership {
  target: NativeTarget
  role: 'owner' | 'member'
  joinedAtMs: number
}

export interface NativeThreadSummary {
  rootMessageId: string
  threadId: string
  replyCount: number
  lastReplyAtMs: number | null
  recentReplierIds: string[]
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

export interface NativeIdentityContext {
  agent: NativeAgentProfile
  target?: NativeTarget
  membershipTarget?: NativeTarget
  members: NativeTargetMember[]
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
  contexts: NativeIdentityContext[]
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
  /** Authoritative snippet of the anchor Message body, resolved by the store. */
  anchorText?: string
}

export interface NativeChangeEvent {
  seq: string
  kind:
    | 'actor_created'
    | 'agent_profile_changed'
    | 'target_created'
    | 'membership_changed'
    | 'thread_follow_changed'
    | 'message_created'
    | 'task_created'
    | 'task_updated'
    | 'activity_done_changed'
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

export interface NativeActivityInboxReply {
  senderName: string
  senderKind: 'user' | 'agent'
  excerpt: string
  atMs: number
}

export interface NativeActivityInboxTask {
  number: string
  status: NativeTask['status']
  assigneeName?: string
}

export interface NativeActivityInboxItem {
  conversationId: string
  targetKind: NativeTarget['kind']
  parentTargetId?: string
  rootMessageId?: string
  targetName: string
  titleKind: 'thread' | 'message'
  title: string
  latestReply?: NativeActivityInboxReply
  lastActivityAtMs: number
  lastActivitySeq: string
  replyCount?: string
  task?: NativeActivityInboxTask
  /** All 视图下:该会话 Done fence 已覆盖最新活动。 */
  done?: boolean
}

export interface NativeActivityInboxPage {
  items: NativeActivityInboxItem[]
  nextCursor?: string
  activeCount: string
}

export interface NativeCollabHandle {
  close(): Promise<void>
  createUser(handle: string, displayName: string): Promise<NativeActor>
  ensureUser(handle: string, displayName: string): Promise<NativeActor>
  createAgent(handle: string, displayName: string, workspacePath: string): Promise<NativeActor>
  createAgentProfile(
    handle: string,
    displayName: string,
    workspacePath: string,
    charter: NativeAgentCharter,
  ): Promise<NativeAgentProfile>
  agentProfile(agentId: string): Promise<NativeAgentProfile>
  listAgentProfiles(actorId: string): Promise<NativeAgentProfile[]>
  updateAgentProfile(
    agentId: string,
    displayName: string,
    charter: NativeAgentCharter,
    expectedVersion: string,
  ): Promise<NativeAgentProfile>
  identityContext(agentId: string, targetId?: string): Promise<NativeIdentityContext>
  deleteAgent(agentId: string): Promise<void>
  createChannel(name: string, creatorId: string): Promise<NativeTarget>
  createDirect(actorId: string, peerId: string): Promise<NativeTarget>
  createThread(rootMessageId: string, actorId: string): Promise<NativeTarget>
  /** tae thread-summaries 等价物：批量 rootMessageIds（≤100）→ 计数+最近 3 个回复者（不含正文/不含无回复/不可见 thread）。 */
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
  bindRuntime(
    agentId: string,
    sessionId: string,
    provider: string,
    model: string,
    preset: string,
  ): Promise<NativeRuntimeBinding>
  runtimeBinding(agentId: string): Promise<NativeRuntimeBinding | undefined>
  updateRuntimePreset(
    agentId: string,
    generation: string,
    sessionId: string,
    preset: string,
  ): Promise<NativeRuntimeBinding>
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
  inboxList(actorId: string, limit: number, cursor?: string, filter?: 'all' | 'unread'): Promise<NativeActivityInboxPage>
  inboxDone(actorId: string, targetId: string, throughSeq: string): Promise<void>
  inboxDoneAll(actorId: string): Promise<number>
  listActors(actorId: string): Promise<NativeActor[]>
  listTargetMembers(actorId: string, targetId: string): Promise<NativeActor[]>
  listTargetMemberships(actorId: string, targetId: string): Promise<NativeTargetMember[]>
  listAgentMemberships(actorId: string, agentId: string): Promise<NativeAgentMembership[]>
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
