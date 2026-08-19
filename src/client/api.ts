import type { ConnectionHandle } from '@deepseek-ai/dsh-client-connection/client'
import type { AgentPresetSummary, AgentProfile } from '../agent-settings-types.ts'
import type { NativeActor, NativeCollabSnapshot, NativeRuntimeBinding } from '../native.ts'

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
  name: string
  presetId: string
}

/** Result of agent.create: the actor, its runtime binding, and the workspace path. */
export interface CreatedAgent {
  actor: NativeActor
  binding: NativeRuntimeBinding
  workspacePath: string
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

  agentPresets(): Promise<AgentPresetSummary[]> {
    return this.call('agent.presets', {})
  }

  agentProfile(agentId: string): Promise<AgentProfile> {
    return this.call('agent.profile', { agentId })
  }

  runtimeBindings(): Promise<NativeRuntimeBinding[]> {
    return this.call('runtime.bindings', {})
  }

  createAgent(request: CreateAgentRequest): Promise<CreatedAgent> {
    return this.call('agent.create', { name: request.name, presetId: request.presetId })
  }

  deleteAgent(agentId: string): Promise<null> {
    return this.call('agent.delete', { agentId })
  }
}
