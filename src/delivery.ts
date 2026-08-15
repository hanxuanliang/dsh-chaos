import { createUserMessage } from '@deepseek-ai/dsh-llm'
import type { CollabRuntimeApi, WarningSink } from './contracts.ts'
import type { RuntimeManager } from './runtime.ts'

const WAKE_TEXT = 'Collab inbox update: new messages are pending. Call message_check to inspect them.'

/** Level-triggered bridge from durable wake watermarks to current DSH Agents. */
export class DeliveryBridge {
  private timer: NodeJS.Timeout | undefined
  private requested = false
  private running: Promise<void> | undefined
  private stopped = true

  constructor(
    private readonly collab: CollabRuntimeApi,
    private readonly runtimes: RuntimeManager,
    private readonly warnings: WarningSink,
    private readonly pollMs: number,
  ) {
    if (!Number.isSafeInteger(pollMs) || pollMs < 50) {
      throw new Error('delivery poll interval must be an integer of at least 50ms')
    }
  }

  start(): void {
    if (!this.stopped) return
    this.stopped = false
    this.timer = setInterval(() => this.kick(), this.pollMs)
    this.timer.unref()
    this.kick()
  }

  kick(): void {
    if (this.stopped) return
    this.requested = true
    if (this.running !== undefined) return
    this.running = this.drain().finally(() => {
      this.running = undefined
      if (!this.stopped && this.requested) this.kick()
    })
  }

  async stop(): Promise<void> {
    if (this.stopped) return
    this.stopped = true
    this.requested = false
    if (this.timer !== undefined) clearInterval(this.timer)
    this.timer = undefined
    await this.running
  }

  async scanOnce(): Promise<void> {
    const wakes = await this.collab.listPendingWakes(1000)
    await Promise.all(wakes.map(async (wake) => {
      const agent = this.runtimes.resolve(wake.binding)
      if (agent === undefined) return
      try {
        const notice = createUserMessage({
          content: [{ type: 'text', text: WAKE_TEXT }],
          source: {
            kind: 'plugin',
            plugin: 'dsh-chaos',
            form: 'notice',
            summary: 'Collab messages pending',
          },
        })
        if (agent.status === 'running') agent.steer(notice)
        else agent.followup(notice)
        await this.collab.markNotified(
          wake.binding.agentId,
          wake.binding.generation,
          wake.binding.sessionId,
          wake.pendingSeq,
        )
      } catch (error) {
        this.warnings.warn(`dsh-chaos: wake failed for Agent ${wake.binding.agentId}`, error)
      }
    }))
  }

  private async drain(): Promise<void> {
    while (!this.stopped && this.requested) {
      this.requested = false
      try {
        await this.scanOnce()
      } catch (error) {
        this.warnings.warn('dsh-chaos: pending wake scan failed', error)
      }
    }
  }
}
