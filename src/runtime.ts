import { randomUUID } from 'node:crypto'
import { isAbsolute } from 'node:path'
import type { Context } from '@deepseek-ai/cordis'
import type { Agent, AgentHandle, AgentRegistry } from '@deepseek-ai/dsh-agent'
import type { AgentPreset, AgentPresets } from '@deepseek-ai/dsh-agent-presets'
import { SessionId } from '@deepseek-ai/dsh-session'
import { installAgentIdentityPrompt } from './agent-prompt.ts'
import { initializeAgentWorkspace } from './agent-workspace.ts'
import type { CollabRuntimeApi, WarningSink } from './contracts.ts'
import type { NativeAgentProfile, NativeRuntimeBinding } from './native.ts'
import { requireSupportedAgentPreset } from './preset-policy.ts'
import { installCollabTools } from './tools.ts'

export interface CreateRuntimeInput {
  agentId: string
  workspacePath: string
  provider: string
  model: string
  preset: string
  sessionId?: string
}

interface RuntimeLease {
  binding: NativeRuntimeBinding
  handle: AgentHandle
}

interface PendingReceipt {
  batchId: string
  binding: NativeRuntimeBinding
}

interface PreparedRuntime {
  preset: AgentPreset
  sessionId: string
}

export type AgentPresetRoster = Pick<AgentPresets, 'mount' | 'resolve'>
export interface PermissionPresetWriter {
  set(session: Agent['session'], preset: string): void
}

const CHAOS_PERMISSION_PRESET = 'danger-full-access'

const sameBinding = (left: NativeRuntimeBinding, right: NativeRuntimeBinding): boolean =>
  left.agentId === right.agentId
  && left.sessionId === right.sessionId
  && left.generation === right.generation

const requireText = (name: string, value: string): void => {
  if (value.trim().length === 0) throw new Error(`${name} must not be blank`)
}

/** Owns every programmatically created top-level DSH AgentHandle. */
export class RuntimeManager {
  private readonly active = new Map<string, RuntimeLease>()
  private readonly identities = new Map<string, NativeAgentProfile>()
  private readonly locks = new Map<string, Promise<void>>()
  private readonly pendingReceipts = new Map<string, Map<string, PendingReceipt>>()
  private closing = false

  constructor(
    private readonly registry: Pick<AgentRegistry, 'create' | 'resume'>,
    private readonly presets: AgentPresetRoster,
    private readonly permissions: PermissionPresetWriter,
    private readonly collab: CollabRuntimeApi,
    private readonly warnings: WarningSink,
  ) {}

  /** Create a fresh long-running Session and publish its binding only after DSH publication succeeds. */
  create(input: CreateRuntimeInput): Promise<NativeRuntimeBinding> {
    return this.withAgentLock(input.agentId, () => this.createLocked(input))
  }

  /** Replace or first-configure one stable Agent under an exact generation fence. */
  reset(input: CreateRuntimeInput, expectedGeneration?: string): Promise<NativeRuntimeBinding> {
    return this.withAgentLock(input.agentId, async () => {
      this.assertActive()
      const current = await this.collab.runtimeBinding(input.agentId)
      if (expectedGeneration === undefined) {
        if (current !== undefined) {
          throw new Error(
            `[runtime_generation_mismatch] Agent ${input.agentId} already has generation ${current.generation}`,
          )
        }
      } else if (current === undefined || current.generation !== expectedGeneration) {
        throw new Error(
          `[runtime_generation_mismatch] Agent ${input.agentId} expected generation ${expectedGeneration}, current generation is ${current?.generation ?? 'none'}`,
        )
      }
      const prepared = await this.prepareRuntime(input)
      try {
        return await this.createPrepared(input, prepared)
      } catch (error) {
        if (current === undefined) throw error
        try {
          await this.resumeLocked(input.agentId, current)
        } catch (resumeError) {
          throw new AggregateError(
            [error, resumeError],
            `failed to replace and recover Agent ${input.agentId}`,
          )
        }
        throw error
      }
    })
  }

  private async createLocked(input: CreateRuntimeInput): Promise<NativeRuntimeBinding> {
    return await this.createPrepared(input, await this.prepareRuntime(input))
  }

  /** Validate and initialize before disturbing the currently active Session. */
  private async prepareRuntime(input: CreateRuntimeInput): Promise<PreparedRuntime> {
    this.assertActive()
    for (const [name, value] of [
      ['agentId', input.agentId],
      ['workspacePath', input.workspacePath],
      ['provider', input.provider],
      ['model', input.model],
      ['preset', input.preset],
    ] as const) requireText(name, value)
    if (!isAbsolute(input.workspacePath)) throw new Error('workspacePath must be absolute')

    const sessionId = input.sessionId ?? randomUUID()
    requireText('sessionId', sessionId)
    const preset = await this.resolvePreset(input.preset)
    const identity = (await this.collab.identityContext(input.agentId)).agent
    if (input.workspacePath !== identity.workspacePath) {
      throw new Error('workspacePath must match the durable Agent Profile')
    }
    this.updateIdentityProjection(identity)
    await initializeAgentWorkspace(input.workspacePath, identity)
    return { preset, sessionId }
  }

  private async createPrepared(
    input: CreateRuntimeInput,
    { preset, sessionId }: PreparedRuntime,
  ): Promise<NativeRuntimeBinding> {
    await this.disposeLease(input.agentId)
    const handle = await this.registry.create({
      sessionId: SessionId(sessionId),
      meta: { cwd: input.workspacePath, agentPreset: preset.id },
      ...(input.provider === 'default'
        ? {}
        : { agentOptions: { provider: input.provider, model: input.model } }),
      setup: async (agentCtx: Context) => {
        await this.presets.mount(agentCtx, preset.id)
        installAgentIdentityPrompt(agentCtx, () => this.requireIdentity(input.agentId))
        installCollabTools(agentCtx, this.collab, this)
      },
    })
    try {
      this.permissions.set(handle.agent.session, CHAOS_PERMISSION_PRESET)
      const binding = await this.collab.bindRuntime(
        input.agentId,
        sessionId,
        input.provider,
        input.model,
        preset.id,
      )
      this.active.set(input.agentId, { binding, handle })
      return binding
    } catch (error) {
      try {
        await handle.dispose()
      } catch (disposeError) {
        throw new AggregateError([error, disposeError], `failed to bind and dispose Agent ${input.agentId}`)
      }
      throw error
    }
  }

  /** Resume every persisted binding independently; one stale Session does not hide healthy peers. */
  async restore(): Promise<void> {
    const bindings = await this.collab.listRuntimeBindings()
    await Promise.all(bindings.map(async (binding) => {
      try {
        await this.resume(binding.agentId)
      } catch (error) {
        this.warnings.warn(`dsh-chaos: failed to resume Agent ${binding.agentId}`, error)
      }
    }))
  }

  /** Resume one persisted Session without incrementing its generation. */
  resume(agentId: string): Promise<NativeRuntimeBinding> {
    return this.withAgentLock(agentId, () => this.resumeLocked(agentId))
  }

  private async resumeLocked(
    agentId: string,
    expectedBinding?: NativeRuntimeBinding,
  ): Promise<NativeRuntimeBinding> {
    this.assertActive()
    let binding = await this.collab.runtimeBinding(agentId)
    if (binding === undefined) throw new Error(`no runtime binding for Agent ${agentId}`)
    if (expectedBinding !== undefined && !sameBinding(binding, expectedBinding)) {
      throw new Error(`runtime binding changed before recovering Agent ${agentId}`)
    }
    const preset = await this.resolvePreset(binding.preset)
    const identity = (await this.collab.identityContext(agentId)).agent
    this.updateIdentityProjection(identity)
    await initializeAgentWorkspace(identity.workspacePath, identity)
    await this.disposeLease(agentId)
    const handle = await this.registry.resume({
      resumeSessionId: SessionId(binding.sessionId),
      agentOptions: { provider: binding.provider, model: binding.model },
      setup: async (agentCtx: Context) => {
        await this.presets.mount(agentCtx, preset.id)
        installAgentIdentityPrompt(agentCtx, () => this.requireIdentity(agentId))
        installCollabTools(agentCtx, this.collab, this)
      },
    })
    try {
      if (preset.id !== binding.preset) {
        handle.agent.session.append('agent-preset/selected', { agentPreset: preset.id })
        binding = await this.collab.updateRuntimePreset(
          binding.agentId,
          binding.generation,
          binding.sessionId,
          preset.id,
        )
      }
      const current = await this.collab.runtimeBinding(agentId)
      if (current === undefined || !sameBinding(current, binding)) {
        throw new Error(`runtime binding changed while resuming Agent ${agentId}`)
      }
      await this.collab.rearmRuntimeWake(
        binding.agentId,
        binding.generation,
        binding.sessionId,
      )
      this.active.set(agentId, { binding, handle })
      return binding
    } catch (error) {
      try {
        await handle.dispose()
      } catch (disposeError) {
        throw new AggregateError([error, disposeError], `failed to resume and dispose Agent ${agentId}`)
      }
      throw error
    }
  }

  /** Stop one process-local AgentHandle while retaining its durable binding for later resume. */
  stop(agentId: string): Promise<void> {
    return this.withAgentLock(agentId, () => this.disposeLease(agentId))
  }

  /** Refresh the live prompt projection after a committed Profile mutation. */
  updateIdentityProjection(profile: NativeAgentProfile): void {
    const current = this.identities.get(profile.actor.id)
    if (current === undefined || BigInt(profile.version) >= BigInt(current.version)) {
      this.identities.set(profile.actor.id, profile)
    }
  }

  forgetIdentity(agentId: string): void {
    this.identities.delete(agentId)
  }

  /** Resolve only the handle matching the exact durable Session generation. */
  resolve(binding: NativeRuntimeBinding): Agent | undefined {
    const lease = this.active.get(binding.agentId)
    return lease !== undefined && sameBinding(lease.binding, binding)
      ? lease.handle.agent
      : undefined
  }

  /** Derive stable collab identity from DSH's trusted execution Agent. */
  async bindingForExecution(agent: Agent): Promise<NativeRuntimeBinding> {
    const binding = await this.collab.runtimeBindingForSession(String(agent.id))
    if (binding === undefined) throw new Error(`DSH Session ${String(agent.id)} is not bound to a collab Agent`)
    const lease = this.active.get(binding.agentId)
    if (lease === undefined || lease.handle.agent !== agent || !sameBinding(lease.binding, binding)) {
      throw new Error(`DSH Session ${String(agent.id)} is not the current runtime generation`)
    }
    return binding
  }

  /** Remember that a tool result must enter a later model request before it is model-seen. */
  recordInboxBatch(binding: NativeRuntimeBinding, batchId: string): void {
    const lease = this.active.get(binding.agentId)
    if (lease === undefined || !sameBinding(lease.binding, binding)) {
      throw new Error(`cannot record inbox batch for stale Agent ${binding.agentId}`)
    }
    let receipts = this.pendingReceipts.get(binding.sessionId)
    if (receipts === undefined) {
      receipts = new Map()
      this.pendingReceipts.set(binding.sessionId, receipts)
    }
    receipts.set(batchId, { batchId, binding })
  }

  /** Confirm batches when iteration of the next fully assembled model request begins. */
  async confirmModelSeen(sessionId: string): Promise<void> {
    const receipts = this.pendingReceipts.get(sessionId)
    if (receipts === undefined || receipts.size === 0) return
    for (const receipt of [...receipts.values()]) {
      try {
        await this.collab.markModelSeen(
          receipt.batchId,
          receipt.binding.agentId,
          receipt.binding.generation,
          receipt.binding.sessionId,
        )
        receipts.delete(receipt.batchId)
      } catch (error) {
        this.warnings.warn(`dsh-chaos: failed to confirm inbox batch ${receipt.batchId}`, error)
      }
    }
    if (receipts.size === 0) this.pendingReceipts.delete(sessionId)
  }

  /** Re-arm an idle Agent whose checked batch never reached another model request. */
  async rearmUnconfirmed(agent: Agent): Promise<void> {
    const lease = [...this.active.values()].find(candidate => candidate.handle.agent === agent)
    if (lease === undefined || !this.pendingReceipts.has(lease.binding.sessionId)) return
    await this.collab.rearmRuntimeWake(
      lease.binding.agentId,
      lease.binding.generation,
      lease.binding.sessionId,
    )
  }

  async close(): Promise<void> {
    if (this.closing) return
    this.closing = true
    await Promise.all(this.locks.values())
    const leases = [...this.active.values()]
    this.pendingReceipts.clear()
    const errors: unknown[] = []
    await Promise.all(leases.map(async (lease) => {
      try {
        await lease.handle.dispose()
        if (this.active.get(lease.binding.agentId) === lease) {
          this.active.delete(lease.binding.agentId)
        }
      } catch (error) {
        errors.push(error)
      }
    }))
    if (errors.length > 0) throw new AggregateError(errors, 'failed to dispose one or more DSH Agents')
    this.identities.clear()
  }

  private async disposeLease(agentId: string): Promise<void> {
    const lease = this.active.get(agentId)
    if (lease === undefined) return
    this.active.delete(agentId)
    this.pendingReceipts.delete(lease.binding.sessionId)
    try {
      await lease.handle.dispose()
    } catch (error) {
      if (!this.active.has(agentId)) this.active.set(agentId, lease)
      throw error
    }
  }

  private async withAgentLock<T>(agentId: string, operation: () => Promise<T>): Promise<T> {
    requireText('agentId', agentId)
    const predecessor = this.locks.get(agentId) ?? Promise.resolve()
    let release!: () => void
    const current = new Promise<void>((resolve) => { release = resolve })
    this.locks.set(agentId, current)
    await predecessor
    try {
      return await operation()
    } finally {
      release()
      if (this.locks.get(agentId) === current) this.locks.delete(agentId)
    }
  }

  private assertActive(): void {
    if (this.closing) throw new Error('dsh-chaos RuntimeManager is closing')
  }

  private requireIdentity(agentId: string): NativeAgentProfile {
    const identity = this.identities.get(agentId)
    if (identity === undefined) throw new Error(`no identity projection for Agent ${agentId}`)
    return identity
  }

  /** Resolve one of the three long-lived collaboration Presets. */
  private async resolvePreset(requested: string): Promise<AgentPreset> {
    return await this.presets.resolve(requireSupportedAgentPreset(requested))
  }
}
