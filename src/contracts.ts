import type {
  NativeInboxBatch,
  NativeMessage,
  NativePendingWake,
  NativeRuntimeBinding,
  NativeSendResult,
  NativeTask,
} from './native.ts'

/** Narrow host contract shared by runtime wiring, delivery, and model tools. */
export interface CollabRuntimeApi {
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
  checkInbox(
    agentId: string,
    generation: string,
    sessionId: string,
    limit: number,
  ): Promise<NativeInboxBatch>
  markModelSeen(
    batchId: string,
    agentId: string,
    generation: string,
    sessionId: string,
  ): Promise<void>
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
  sendMessage(input: {
    targetId: string
    authorId: string
    clientRequestId: string
    text: string
  }): Promise<NativeSendResult>
  readMessage(actorId: string, targetId: string, messageId: string): Promise<NativeMessage>
  readMessages(actorId: string, targetId: string, afterSeq: string, limit: number): Promise<NativeMessage[]>
}

export interface WarningSink {
  warn(message: string, error?: unknown): void
}
