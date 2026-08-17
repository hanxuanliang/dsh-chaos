import { randomUUID } from 'node:crypto'
import { isAbsolute } from 'node:path'
import type { Context } from '@deepseek-ai/cordis'
import type { Agent, AgentHandle, AgentRegistry } from '@deepseek-ai/dsh-agent'
import { SessionId } from '@deepseek-ai/dsh-session'
import type { CollabRuntimeApi, WarningSink } from './contracts.ts'
import type { NativeRuntimeBinding } from './native.ts'
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
  private readonly locks = new Map<string, Promise<void>>()
  private readonly pendingReceipts = new Map<string, Map<string, PendingReceipt>>()
  private closing = false

  constructor(
    private readonly registry: Pick<AgentRegistry, 'create' | 'resume'>,
    private readonly collab: CollabRuntimeApi,
    private readonly warnings: WarningSink,
  ) {}

  /** Create a fresh long-running Session and publish its binding only after DSH publication succeeds. */
  create(input: CreateRuntimeInput): Promise<NativeRuntimeBinding> {
    return this.withAgentLock(input.agentId, async () => {
      this.assertActive()
      for (const [name, value] of [
        ['agentId', input.agentId],
        ['workspacePath', input.workspacePath],
        ['provider', input.provider],
        ['model', input.model],
        ['preset', input.preset],
      ] as const) requireText(name, value)
      if (!isAbsolute(input.workspacePath)) throw new Error('workspacePath must be absolute')

      await this.disposeLease(input.agentId)
      const sessionId = input.sessionId ?? randomUUID()
      requireText('sessionId', sessionId)
      const handle = await this.registry.create({
        sessionId: SessionId(sessionId),
        meta: { cwd: input.workspacePath, agentPreset: input.preset },
        ...(input.provider === 'default'
          ? {}
          : { agentOptions: { provider: input.provider, model: input.model } }),
        setup: (agentCtx: Context) => installCollabTools(agentCtx, this.collab, this),
      })
      try {
        const binding = await this.collab.bindRuntime(
          input.agentId,
          sessionId,
          input.provider,
          input.model,
          input.preset,
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
    })
  }

  /** Replace one stable Agent's Session generation while preserving its collab identity. */
  reset(input: CreateRuntimeInput): Promise<NativeRuntimeBinding> {
    return this.create(input)
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
    return this.withAgentLock(agentId, async () => {
      this.assertActive()
      const binding = await this.collab.runtimeBinding(agentId)
      if (binding === undefined) throw new Error(`no runtime binding for Agent ${agentId}`)
      await this.disposeLease(agentId)
      const handle = await this.registry.resume({
        resumeSessionId: SessionId(binding.sessionId),
        agentOptions: { provider: binding.provider, model: binding.model },
        setup: (agentCtx: Context) => installCollabTools(agentCtx, this.collab, this),
      })
      try {
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
    })
  }

  /** Stop one process-local AgentHandle while retaining its durable binding for later resume. */
  stop(agentId: string): Promise<void> {
    return this.withAgentLock(agentId, () => this.disposeLease(agentId))
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
}
