import type { ClientConnectionRpc } from '@deepseek-ai/dsh-client-connection/client'
import type { HostObservable } from '@deepseek-ai/dsh-client-ui-slots'
import type {
  NativeActivityInboxItem,
  NativeActivityInboxPage,
  NativeActor,
  NativeCollabSnapshot,
  NativeMessage,
  NativeMessageTail,
  NativeSendResult,
  NativeTarget,
  NativeTask,
} from '../native.ts'
import type { CollabDomainResult } from '../remote.ts'

const RPC_CHANNEL = '/dsh-chaos'
const EVENTS_PATH = '/dsh-chaos/events'

export interface ThreadPreviewReply {
  /** Stable message id, used as the preview row key. */
  id: string
  authorId: string
  text: string
  createdAtMs: number
}

export interface ThreadPreview {
  /** Exact total reply count from the authoritative tail RPC. */
  count: number
  /** True latest replies, ascending. */
  latest: ThreadPreviewReply[]
}

export interface ChaosInboxState {
  status: 'idle' | 'loading' | 'ready' | 'error'
  items: readonly NativeActivityInboxItem[]
  nextCursor?: string
  /** Authoritative active-conversation count from inbox.list (decimal string). */
  activeCount: string
}

export interface ChaosClientState {
  status: 'cold' | 'loading' | 'ready' | 'error'
  stream: 'idle' | 'connecting' | 'connected' | 'reconnecting'
  /** Docked right-side collaboration surface (Activity list / conversation). */
  dock: 'closed' | 'open'
  cursor: string
  actor?: NativeActor
  actors: readonly NativeActor[]
  targets: readonly NativeTarget[]
  followedThreadIds: readonly string[]
  /** Active members of the selected Channel from the membership projection. */
  members: readonly NativeActor[]
  /** Tasks of the selected conversation. */
  tasks: readonly NativeTask[]
  selectedTargetId?: string
  messages: readonly NativeMessage[]
  /** Inline reply previews for threads of the selected target, keyed by thread target id. */
  threadPreviews: Record<string, ThreadPreview>
  /** In-dock Thread view: keeps reading (and SSE) even after unfollow. */
  threadPanelId?: string
  threadPanelMessages: readonly NativeMessage[]
  /** Authoritative Activity inbox page (dock list view). */
  inbox: ChaosInboxState
  error: string | undefined
}

const INITIAL_STATE: ChaosClientState = {
  status: 'cold',
  stream: 'idle',
  dock: 'closed',
  cursor: '0',
  actors: [],
  targets: [],
  followedThreadIds: [],
  members: [],
  tasks: [],
  messages: [],
  threadPreviews: {},
  threadPanelMessages: [],
  inbox: { status: 'idle', items: [], activeCount: '0' },
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
  private panelEpoch = 0
  private inboxEpoch = 0

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

  /** Footer Activity entry: open the dock on the Activity list. */
  openActivity(): void {
    this.publish(this.listViewState())
    void this.ensure()
  }

  /** Conversation → Activity list, staying inside the dock. */
  backToList(): void {
    this.publish(this.listViewState())
  }

  closeDock(): void {
    if (this.state.dock === 'closed') return
    this.publish({ ...this.state, dock: 'closed' })
  }

  private listViewState(): ChaosClientState {
    const next: ChaosClientState = {
      ...this.state,
      dock: 'open',
      messages: [],
      tasks: [],
      members: [],
      threadPreviews: {},
      threadPanelMessages: [],
    }
    delete next.selectedTargetId
    delete next.threadPanelId
    return next
  }

  /**
   * Open a conversation in the docked right-side panel (Activity card landing
   * surface).
   */
  async openDock(targetId: string): Promise<void> {
    if (!this.state.targets.some(target => target.id === targetId)) return
    const next: ChaosClientState = {
      ...this.state,
      dock: 'open',
      selectedTargetId: targetId,
      messages: [],
      tasks: [],
      members: [],
      threadPanelMessages: [],
    }
    delete next.threadPanelId
    this.publish(next)
    await this.reloadTarget(targetId)
  }

  /**
   * Load the authoritative Activity inbox. Without a cursor the first page is
   * reloaded (sized to cover already-loaded items so a refresh after a Done or
   * a change event does not clobber appended pages); with a cursor the next
   * page is appended.
   */
  async loadInbox(cursor?: string): Promise<void> {
    if (this.disposed) return
    const epoch = ++this.inboxEpoch
    const append = cursor !== undefined
    const previous = this.state.inbox
    if (!append && previous.status === 'idle') {
      this.publish({ ...this.state, inbox: { ...previous, status: 'loading' } })
    }
    try {
      const page = await this.call<NativeActivityInboxPage>('inbox.list', {
        limit: append ? 20 : Math.min(50, Math.max(20, previous.items.length)),
        ...(cursor === undefined ? {} : { cursor }),
      })
      if (epoch !== this.inboxEpoch || this.disposed) return
      const next: ChaosInboxState = {
        status: 'ready',
        items: append ? [...this.state.inbox.items, ...page.items] : page.items,
        activeCount: page.activeCount,
      }
      if (page.nextCursor !== undefined) next.nextCursor = page.nextCursor
      this.publish({ ...this.state, inbox: next })
    } catch {
      if (epoch !== this.inboxEpoch || this.disposed) return
      this.publish({ ...this.state, inbox: { ...this.state.inbox, status: 'error' } })
    }
  }

  /**
   * Mark a conversation Done through its latest seq: it leaves the inbox until
   * a newer message revives it. The item is removed optimistically; the
   * activity_done_changed SSE event re-syncs the authoritative page.
   */
  async markInboxDone(targetId: string, throughSeq: string): Promise<void> {
    await this.call('inbox.done', { targetId, throughSeq })
    const inbox = this.state.inbox
    if (inbox.status === 'idle') return
    this.publish({
      ...this.state,
      inbox: {
        ...inbox,
        items: inbox.items.filter(item => item.conversationId !== targetId),
      },
    })
  }

  async createThread(rootMessageId: string): Promise<void> {
    const target = await this.call<NativeTarget>('thread.create', { rootMessageId })
    await this.reloadProjection()
    // Threads open in the in-dock Thread view; the main conversation stays put.
    await this.openThreadPanel(target.id)
  }

  async followThread(threadTargetId: string): Promise<void> {
    await this.call('thread.follow', { threadTargetId })
    await this.reloadProjection()
  }

  async unfollowThread(threadTargetId: string): Promise<void> {
    await this.call('thread.unfollow', { threadTargetId })
    await this.reloadProjection()
  }

  async send(text: string): Promise<NativeSendResult> {
    const targetId = this.state.selectedTargetId
    if (targetId === undefined) throw new Error('请先选择一个协作目标')
    return await this.sendTo(targetId, text)
  }

  private async sendTo(targetId: string, text: string): Promise<NativeSendResult> {
    const sent = await this.call<NativeSendResult>('message.send', {
      targetId,
      requestId: crypto.randomUUID(),
      text,
    })
    const target = this.state.targets.find(candidate => candidate.id === targetId)
    if (target?.kind === 'thread') await this.reloadProjection()
    else await this.reloadTarget(targetId)
    return sent
  }

  async openThreadPanel(threadTargetId: string): Promise<void> {
    let thread = this.state.targets.find(
      target => target.id === threadTargetId && target.kind === 'thread',
    )
    if (thread === undefined) return
    if (thread.parentTargetId !== undefined && this.state.selectedTargetId !== thread.parentTargetId) {
      await this.openDock(thread.parentTargetId)
      thread = this.state.targets.find(
        target => target.id === threadTargetId && target.kind === 'thread',
      )
      if (thread === undefined) return
    }
    this.publish({
      ...this.state,
      dock: 'open',
      threadPanelId: thread.id,
      threadPanelMessages: [],
    })
    await this.reloadThreadPanel()
  }

  closeThreadPanel(): void {
    if (this.state.threadPanelId === undefined) return
    this.panelEpoch += 1
    const next: ChaosClientState = { ...this.state, threadPanelMessages: [] }
    delete next.threadPanelId
    this.publish(next)
  }

  async sendToThread(text: string): Promise<void> {
    const threadTargetId = this.state.threadPanelId
    if (threadTargetId === undefined) throw new Error('没有打开的 Thread')
    await this.call('message.send', {
      targetId: threadTargetId,
      requestId: crypto.randomUUID(),
      text,
    })
    // Sending in a thread can change follow state and the parent preview,
    // so the whole projection (and with it the main target) reloads.
    await this.reloadProjection()
  }

  async createTask(messageId: string): Promise<void> {
    await this.call<NativeTask>('task.create', { messageId })
    await this.reloadProjection()
  }

  dispose(): void {
    this.disposed = true
    this.started = false
    this.projectionRequested = false
    this.loadEpoch += 1
    this.panelEpoch += 1
    this.inboxEpoch += 1
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
      : undefined
    const selectionChanged = selectedTargetId !== this.state.selectedTargetId
    const threadPanelId = this.state.threadPanelId !== undefined
      && snapshot.targets.some(target => target.id === this.state.threadPanelId)
      ? this.state.threadPanelId
      : undefined
    const next: ChaosClientState = {
      ...this.state,
      status: 'ready',
      cursor: snapshot.cursor,
      actor: snapshot.actor,
      actors,
      targets: snapshot.targets,
      followedThreadIds: snapshot.followedThreadIds,
      tasks: selectedTargetId === undefined || selectionChanged ? [] : this.state.tasks,
      messages: selectedTargetId === undefined || selectionChanged ? [] : this.state.messages,
      members: selectedTargetId === undefined || selectionChanged ? [] : this.state.members,
      threadPanelMessages: threadPanelId === undefined ? [] : this.state.threadPanelMessages,
      error: undefined,
    }
    delete next.selectedTargetId
    delete next.threadPanelId
    if (selectedTargetId !== undefined) next.selectedTargetId = selectedTargetId
    if (threadPanelId !== undefined) next.threadPanelId = threadPanelId
    this.publish(next)
    if (selectedTargetId !== undefined) await this.reloadTarget(selectedTargetId)
    // The open Thread view keeps reading and keeps receiving SSE refreshes
    // even after its follow is removed.
    if (threadPanelId !== undefined) await this.reloadThreadPanel()
    return snapshot
  }

  private async reloadTarget(targetId: string): Promise<void> {
    const epoch = ++this.loadEpoch
    try {
      const kind = this.state.targets.find(target => target.id === targetId)?.kind
      const [tail, tasks, members] = await Promise.all([
        this.call<NativeMessageTail>('history.tail', { targetId, limit: 100 }),
        this.call<NativeTask[]>('tasks', { targetId }),
        kind === 'channel'
          ? this.call<NativeActor[]>('target.members', { targetId })
          : Promise.resolve<NativeActor[]>([]),
      ])
      if (epoch !== this.loadEpoch || this.state.selectedTargetId !== targetId) return
      this.publish({
        ...this.state,
        status: 'ready',
        messages: tail.messages,
        tasks,
        members,
        error: undefined,
      })
      await this.loadThreadPreviews(targetId, tail.messages, epoch)
    } catch (error) {
      if (epoch !== this.loadEpoch) return
      this.publish({ ...this.state, status: 'error', error: messageOf(error) })
    }
  }

  /**
   * Inline reply previews come from the authoritative per-thread tail RPC
   * (exact count + true latest replies in one consistent read), never from
   * the already-loaded parent messages or a frontend guess.
   */
  private async loadThreadPreviews(
    targetId: string,
    messages: readonly NativeMessage[],
    epoch: number,
  ): Promise<void> {
    const rootIds = new Set(messages.map(message => message.id))
    const threads = this.state.targets.filter(
      target =>
        target.kind === 'thread'
        && target.parentTargetId === targetId
        && target.rootMessageId !== undefined
        && rootIds.has(target.rootMessageId),
    )
    try {
      const entries = await Promise.all(
        threads.map(async thread => {
          const tail = await this.call<NativeMessageTail>('history.tail', {
            targetId: thread.id,
            limit: 2,
          })
          const preview: ThreadPreview = {
            count: Number(tail.count),
            latest: tail.messages.map(message => ({
              id: message.id,
              authorId: message.authorId,
              text: message.text,
              createdAtMs: message.createdAtMs,
            })),
          }
          return [thread.id, preview] as const
        }),
      )
      if (epoch !== this.loadEpoch || this.state.selectedTargetId !== targetId) return
      this.publish({ ...this.state, threadPreviews: Object.fromEntries(entries) })
    } catch {
      // Preview failure must not break the main message list; keep stale/empty.
    }
  }

  private async reloadThreadPanel(): Promise<void> {
    const threadPanelId = this.state.threadPanelId
    if (threadPanelId === undefined) return
    const epoch = ++this.panelEpoch
    try {
      const tail = await this.call<NativeMessageTail>('history.tail', {
        targetId: threadPanelId,
        limit: 100,
      })
      if (epoch !== this.panelEpoch || this.state.threadPanelId !== threadPanelId) return
      this.publish({ ...this.state, threadPanelMessages: tail.messages })
    } catch {
      if (epoch !== this.panelEpoch) return
      this.publish({ ...this.state, threadPanelMessages: [] })
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
      if (this.source !== source) return
      void this.refresh()
      // message_created/activity_done_changed both arrive as a coarse change;
      // once the inbox has been loaded it re-reads its authoritative first page.
      if (this.state.inbox.status !== 'idle') void this.loadInbox()
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

/**
 * Draft clearing after a successful send: only an untouched draft is
 * cleared. Compare against the raw snapshot taken at submit time — not the
 * trimmed payload — so a trailing newline does not look like new typing.
 * Anything typed while the request was in flight belongs to the next
 * message and must survive the earlier request's completion.
 */
export function resolveSentDraft(
  drafts: Readonly<Record<string, string>>,
  key: string,
  sentRaw: string,
): Readonly<Record<string, string>> {
  return drafts[key] === sentRaw ? { ...drafts, [key]: '' } : drafts
}
