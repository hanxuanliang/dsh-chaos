import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type {
  AgentMembership,
  AgentPresetSummary,
  AgentProfile,
  AgentWorkspaceEntry,
  AgentWorkspaceFile,
  CreatedAgent,
} from '../agent-settings-types.ts'
import type {
  NativeActor,
  NativeCollabSnapshot,
  NativeMessage,
  NativeMessageTail,
  NativeRuntimeBinding,
  NativeSendResult,
  NativeTarget,
  NativeTask,
  NativeThreadSummary,
  NativeActivityInboxPage,
} from '../native.ts'

/** RPC channel exposed by the host half (COLLAB_RPC_CHANNEL in src/remote.ts). */
export const CHAOS_RPC_CHANNEL = '/dsh-chaos'

/**
 * Domain envelope, re-declared client-side: src/remote.ts imports node:http
 * and must stay out of the browser bundle. The host handler wraps this inside
 * the transport envelope, so a wire response carries two result layers.
 */
type CollabDomainResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: { code: string; message: string } }

export interface CreateAgentRequest {
  displayName: string
  handle: string
  description: string
  provider: string
  model: string
  presetId: string
}

/** Host model catalog group (apiproxy `llm.models` ModelProviderGroup). */
export interface LlmModelGroup {
  id: string
  name: string
  models: { id: string; name: string; description?: string }[]
}

/** Thin typed caller over the '/dsh-chaos' RPC channel. */
export class ChaosClient {
  constructor(private readonly connection: ConnectionHandle) {}

  /** Unwrap the transport envelope first, then the host's domain envelope. */
  private async call<T>(endpoint: string, payload: unknown): Promise<T> {
    const response = await this.connection.rpc.call(CHAOS_RPC_CHANNEL, endpoint, payload)
    if (!response.ok) throw new Error(response.error.message)
    const result = response.value as CollabDomainResult<T>
    if (!result.ok) throw new Error(result.error.message)
    return result.value
  }

  snapshot(): Promise<NativeCollabSnapshot> {
    return this.call('snapshot', {})
  }

  actors(): Promise<NativeActor[]> {
    return this.call('actors', {})
  }

  // --- P0-2 channel/message/task surface (switch cases in src/remote.ts) ---

  targetMembers(targetId: string): Promise<NativeActor[]> {
    return this.call('target.members', { targetId })
  }

  channelCreate(name: string): Promise<NativeTarget> {
    return this.call('channel.create', { name })
  }

  memberAdd(targetId: string, memberId: string): Promise<null> {
    return this.call('member.add', { targetId, memberId })
  }

  /** Forward-only page (no before-cursor exists); `afterSeq` is a decimal string. */
  history(targetId: string, afterSeq: string, limit: number): Promise<NativeMessage[]> {
    return this.call('history', { targetId, afterSeq, limit })
  }

  /** `count` is the exact total (decimal string); `messages` is the latest page, ascending. */
  historyTail(targetId: string, limit: number): Promise<NativeMessageTail> {
    return this.call('history.tail', { targetId, limit })
  }

  /** Idempotent on (author, requestId); a replay returns the stored row with `replayed: true`. */
  messageSend(targetId: string, requestId: string, text: string): Promise<NativeSendResult> {
    return this.call('message.send', { targetId, requestId, text })
  }

  /** Idempotent for an already-tasked message (returns the existing Task). */
  /** Idempotent per root message (crates create_thread returns the existing row). */
  threadCreate(rootMessageId: string): Promise<NativeTarget> {
    return this.call('thread.create', { rootMessageId })
  }

  /** tae 等价物的 batch 预览：≤100 root → 计数+最近 3 个回复者（含头像组人面）。 */
  threadSummaries(rootMessageIds: string[]): Promise<NativeThreadSummary[]> {
    return this.call('thread.summaries', { rootMessageIds })
  }

  /** crates inbox_list: 活动会话(page)——Done 是 per-actor done_through_seq 语义,新来活动自动复活。 */
  inboxList(limit = 30, cursor?: string): Promise<NativeActivityInboxPage> {
    return this.call('inbox.list', cursor === undefined ? { limit } : { limit, cursor })
  }

  /** inbox_done(targetId + throughSeq 由行上的 lastActivitySeq 给定——按页面拍时结构调用,不取最新)。 */
  inboxDone(targetId: string, throughSeq: string): Promise<null> {
    return this.call('inbox.done', { targetId, throughSeq })
  }

  taskCreate(messageId: string): Promise<NativeTask> {
    return this.call('task.create', { messageId })
  }

  tasks(targetId?: string): Promise<NativeTask[]> {
    return this.call('tasks', targetId === undefined ? {} : { targetId })
  }

  /** todo → in_progress with an assignee attached (fixed-principal actor). */
  taskClaim(messageId: string, actorId?: string): Promise<NativeTask> {
      return this.call('task.claim', actorId === undefined ? { messageId } : { messageId, actorId })
    }

  /** Release own claim back to the pool (fixed-principal, version-fenced). */
  taskUnclaim(messageId: string, expectedVersion: string): Promise<NativeTask> {
    return this.call('task.unclaim', { messageId, expectedVersion })
  }

  taskUpdateStatus(messageId: string, status: NativeTask['status'], expectedVersion: string): Promise<NativeTask> {
    return this.call('task.update', { messageId, status, expectedVersion })
  }

  agentPresets(): Promise<AgentPresetSummary[]> {
    return this.call('agent.presets', {})
  }

  /**
   * Host model catalog (`llm.models`, one call, all provider groups) — the
   * same data the host Models settings page renders. Note: this rides the
   * host apiproxy, not the '/dsh-chaos' channel, so only the transport
   * envelope needs unwrapping.
   */
  async modelCatalog(): Promise<{ groups: LlmModelGroup[]; failures: unknown[] }> {
    const response = await this.connection.api.llm.models({})
    const result = response.result
    if (!result.ok) throw new Error(result.error.message)
    return result.value as { groups: LlmModelGroup[]; failures: unknown[] }
  }

  agentProfile(agentId: string): Promise<AgentProfile> {
    return this.call('agent.profile', { agentId })
  }

  agentProfiles(): Promise<AgentProfile[]> {
    return this.call('agent.profiles', {})
  }

  updateAgentProfile(
    agentId: string,
    displayName: string,
    description: string,
    expectedProfileVersion: string,
  ): Promise<AgentProfile> {
    return this.call('agent.profile.update', {
      agentId,
      displayName,
      description,
      expectedProfileVersion,
    })
  }

  replaceAgentRuntime(
    agentId: string,
    provider: string,
    model: string,
    presetId: string,
    expectedGeneration?: string,
  ): Promise<NativeRuntimeBinding> {
    return this.call('agent.runtime.replace', {
      agentId,
      provider,
      model,
      presetId,
      ...(expectedGeneration === undefined ? {} : { expectedGeneration }),
    })
  }

  agentMemberships(agentId: string): Promise<AgentMembership[]> {
    return this.call('agent.memberships', { agentId })
  }

  agentWorkspace(agentId: string, dirPath: string): Promise<AgentWorkspaceEntry[]> {
    return this.call('agent.workspace.list', { agentId, dirPath, includeHidden: false })
  }

  agentWorkspaceFile(agentId: string, path: string): Promise<AgentWorkspaceFile> {
    return this.call('agent.workspace.read', { agentId, path })
  }

  runtimeBindings(): Promise<NativeRuntimeBinding[]> {
    return this.call('runtime.bindings', {})
  }

  createAgent(request: CreateAgentRequest): Promise<CreatedAgent> {
    return this.call('agent.create', request)
  }

  deleteAgent(agentId: string): Promise<null> {
    return this.call('agent.delete', { agentId })
  }
}
