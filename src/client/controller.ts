import type { ClientConnectionRpc } from '@deepseek-ai/dsh-client-connection/client'
import type { HostObservable } from '@deepseek-ai/dsh-client-ui-slots'
import type {
  NativeActor,
  NativeCollabSnapshot,
  NativeMessage,
  NativeTask,
  NativeTarget,
} from '../native.ts'
import type { CollabDomainResult } from '../remote.ts'

const RPC_CHANNEL = '/dsh-chaos'
const EVENTS_PATH = '/dsh-chaos/events'

export interface ChaosClientState {
  status: 'cold' | 'loading' | 'ready' | 'error'
  stream: 'idle' | 'connecting' | 'connected' | 'reconnecting'
  surface: 'closed' | 'peek' | 'workspace'
  cursor: string
  actor?: NativeActor
  actors: readonly NativeActor[]
  targets: readonly NativeTarget[]
  followedThreadIds: readonly string[]
  allTasks: readonly NativeTask[]
  tasks: readonly NativeTask[]
  selectedTargetId?: string
  messages: readonly NativeMessage[]
  error: string | undefined
}

const INITIAL_STATE: ChaosClientState = {
  status: 'cold',
  stream: 'idle',
  surface: 'closed',
  cursor: '0',
  actors: [],
  targets: [],
  followedThreadIds: [],
  allTasks: [],
  tasks: [],
  messages: [],
  error: undefined,
}

/** Browser object layer over the fixed-principal RPC and durable SSE cursor. */
export class ChaosClientController implements HostObservable<ChaosClientState> {
  private state = INITIAL_STATE
  private readonly listeners = new Set<() => void>()
  private source: EventSource | undefined
  private started = false
  private disposed = false
  private projectionRequested = false
  private projectionRunner: Promise<NativeCollabSnapshot> | undefined
  private loadEpoch = 0

  constructor(private readonly rpc: ClientConnectionRpc) {}

  getSnapshot(): ChaosClientState {
    return this.state
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  async ensure(): Promise<void> {
    if (this.started || this.disposed) return
    this.started = true
    this.publish({ ...this.state, status: 'loading', error: undefined })
    try {
      const snapshot = await this.reloadProjection()
      if (!this.disposed) this.connectEvents(snapshot.cursor)
    } catch (error) {
      if (this.disposed) return
      this.started = false
      this.publish({ ...this.state, status: 'error', error: messageOf(error) })
    }
  }

  async refresh(): Promise<void> {
    if (this.disposed) return
    try {
      const snapshot = await this.reloadProjection()
      if (this.started && this.source === undefined) this.connectEvents(snapshot.cursor)
    } catch (error) {
      this.publish({ ...this.state, status: 'error', error: messageOf(error) })
    }
  }

  togglePeek(): void {
    const surface = this.state.surface === 'peek' ? 'closed' : 'peek'
    this.publish({ ...this.state, surface })
  }

  openWorkspace(): void {
    this.publish({ ...this.state, surface: 'workspace' })
  }

  closeSurface(): void {
    this.publish({ ...this.state, surface: 'closed' })
  }

  async selectTarget(targetId: string): Promise<void> {
    if (!this.state.targets.some(target => target.id === targetId)) return
    this.publish({ ...this.state, selectedTargetId: targetId, messages: [], tasks: [] })
    await this.reloadTarget(targetId)
  }

  async createChannel(name: string): Promise<void> {
    const target = await this.call<NativeTarget>('channel.create', { name })
    await this.reloadProjection()
    await this.selectTarget(target.id)
  }

  async createDirect(peerId: string): Promise<void> {
    const target = await this.call<NativeTarget>('direct.create', { peerId })
    await this.reloadProjection()
    await this.selectTarget(target.id)
  }

  async addMember(targetId: string, memberId: string): Promise<void> {
    await this.call('member.add', { targetId, memberId })
    await this.reloadProjection()
  }

  async createThread(rootMessageId: string): Promise<void> {
    const target = await this.call<NativeTarget>('thread.create', { rootMessageId })
    await this.reloadProjection()
    await this.selectTarget(target.id)
  }

  async followThread(threadTargetId: string): Promise<void> {
    await this.call('thread.follow', { threadTargetId })
    await this.reloadProjection()
  }

  async unfollowThread(threadTargetId: string): Promise<void> {
    await this.call('thread.unfollow', { threadTargetId })
    await this.reloadProjection()
  }

  async send(text: string): Promise<void> {
    const targetId = this.state.selectedTargetId
    if (targetId === undefined) throw new Error('请先选择一个协作目标')
    await this.call('message.send', {
      targetId,
      requestId: crypto.randomUUID(),
      text,
    })
    const target = this.state.targets.find(candidate => candidate.id === targetId)
    if (target?.kind === 'thread') await this.reloadProjection()
    else await this.reloadTarget(targetId)
  }

  async createTask(messageId: string): Promise<void> {
    await this.call<NativeTask>('task.create', { messageId })
    await this.reloadProjection()
  }

  async claimTask(messageId: string): Promise<void> {
    await this.call<NativeTask>('task.claim', { messageId })
    await this.reloadProjection()
  }

  async unclaimTask(task: NativeTask): Promise<void> {
    await this.call<NativeTask>('task.unclaim', {
      messageId: task.messageId,
      expectedVersion: task.version,
    })
    await this.reloadProjection()
  }

  async updateTask(task: NativeTask, status: NativeTask['status']): Promise<void> {
    await this.call<NativeTask>('task.update', {
      messageId: task.messageId,
      status,
      expectedVersion: task.version,
    })
    await this.reloadProjection()
  }

  dispose(): void {
    this.disposed = true
    this.started = false
    this.projectionRequested = false
    this.loadEpoch += 1
    this.source?.close()
    this.source = undefined
    this.listeners.clear()
  }

  private reloadProjection(): Promise<NativeCollabSnapshot> {
    if (this.disposed) return Promise.reject(new Error('dsh-chaos Client 已停止'))
    this.projectionRequested = true
    if (this.projectionRunner !== undefined) return this.projectionRunner

    const runner = this.drainProjectionReloads()
    this.projectionRunner = runner
    void runner.finally(() => {
      if (this.projectionRunner === runner) this.projectionRunner = undefined
    }).catch(() => {})
    return runner
  }

  private async drainProjectionReloads(): Promise<NativeCollabSnapshot> {
    let latest: NativeCollabSnapshot | undefined
    do {
      this.projectionRequested = false
      latest = await this.loadProjectionOnce()
    } while (this.projectionRequested && !this.disposed)
    if (latest === undefined) throw new Error('dsh-chaos Client 已停止')
    return latest
  }

  private async loadProjectionOnce(): Promise<NativeCollabSnapshot> {
    const [snapshot, actors] = await Promise.all([
      this.call<NativeCollabSnapshot>('snapshot', {}),
      this.call<NativeActor[]>('actors', {}),
    ])
    if (this.disposed) throw new Error('dsh-chaos Client 已停止')
    const selectedTargetId = this.state.selectedTargetId !== undefined
      && snapshot.targets.some(target => target.id === this.state.selectedTargetId)
      ? this.state.selectedTargetId
      : snapshot.targets[0]?.id
    const selectionChanged = selectedTargetId !== this.state.selectedTargetId
    const next: ChaosClientState = {
      ...this.state,
      status: 'ready',
      cursor: snapshot.cursor,
      actor: snapshot.actor,
      actors,
      targets: snapshot.targets,
      followedThreadIds: snapshot.followedThreadIds,
      allTasks: snapshot.tasks,
      tasks: selectedTargetId === undefined || selectionChanged ? [] : this.state.tasks,
      messages: selectedTargetId === undefined || selectionChanged ? [] : this.state.messages,
      error: undefined,
    }
    delete next.selectedTargetId
    if (selectedTargetId !== undefined) next.selectedTargetId = selectedTargetId
    this.publish(next)
    if (selectedTargetId !== undefined) await this.reloadTarget(selectedTargetId)
    return snapshot
  }

  private async reloadTarget(targetId: string): Promise<void> {
    const epoch = ++this.loadEpoch
    try {
      const [messages, tasks] = await Promise.all([
        this.call<NativeMessage[]>('history', { targetId, afterSeq: '0', limit: 100 }),
        this.call<NativeTask[]>('tasks', { targetId }),
      ])
      if (epoch !== this.loadEpoch || this.state.selectedTargetId !== targetId) return
      this.publish({ ...this.state, status: 'ready', messages, tasks, error: undefined })
    } catch (error) {
      if (epoch !== this.loadEpoch) return
      this.publish({ ...this.state, status: 'error', error: messageOf(error) })
    }
  }

  private connectEvents(cursor: string): void {
    if (this.disposed) return
    this.source?.close()
    this.publish({ ...this.state, stream: 'connecting' })
    const source = new EventSource(`${EVENTS_PATH}?cursor=${encodeURIComponent(cursor)}`)
    this.source = source
    source.onopen = () => {
      if (this.source === source) this.publish({ ...this.state, stream: 'connected' })
    }
    source.onerror = () => {
      if (this.source === source) this.publish({ ...this.state, stream: 'reconnecting' })
    }
    source.addEventListener('change', () => {
      if (this.source === source) void this.refresh()
    })
    source.addEventListener('resync_required', () => {
      if (this.source === source) void this.resyncEvents(source)
    })
  }

  private async resyncEvents(source: EventSource): Promise<void> {
    if (this.source !== source || this.disposed) return
    source.close()
    this.source = undefined
    this.publish({ ...this.state, stream: 'connecting', error: undefined })
    try {
      const snapshot = await this.reloadProjection()
      if (!this.disposed && this.source === undefined) this.connectEvents(snapshot.cursor)
    } catch (error) {
      if (!this.disposed && this.source === undefined) {
        this.publish({
          ...this.state,
          status: 'error',
          stream: 'idle',
          error: messageOf(error),
        })
      }
    }
  }

  private async call<T>(endpoint: string, payload: unknown): Promise<T> {
    const carrier = await this.rpc.call(RPC_CHANNEL, endpoint, payload)
    if (!carrier.ok) throw new Error(carrier.error.message)
    const domain = carrier.value as CollabDomainResult<T>
    if (domain === null || typeof domain !== 'object' || typeof domain.ok !== 'boolean') {
      throw new Error('dsh-chaos Remote 返回了无效结果')
    }
    if (!domain.ok) throw new Error(`${domain.error.code}: ${domain.error.message}`)
    return domain.value
  }

  private publish(next: ChaosClientState): void {
    if (this.disposed) return
    if (Object.is(this.state, next)) return
    this.state = next
    for (const listener of [...this.listeners]) listener()
  }
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
