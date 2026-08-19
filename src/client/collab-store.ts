/**
 * Collab data store for the P0-2 panel (channels + message stream + tasks).
 * Shape follows UseSyncExternalStore (subscribe/getSnapshot), deliberately the
 * same pattern as panel-controller.ts; merge semantics follow plocal-web's
 * mergeMessagesBySeq (id-dedupe, seq-ascending).
 *
 * Backend truths honored here (see docs/p02-implementation-brief.md):
 * - All seq/count/version wire values are decimal strings (crates napi bridge).
 * - No read markers / unread counters exist server-side: unread = tail.count
 *   minus a per-channel localStorage marker (`dsh-chaos:read:<targetId>`),
 *   written whenever a channel is (re)entered or appended to while active.
 * - history has afterSeq only (no before-cursor), so "load older" pages
 *   forward from seq 0 in 100-message chunks until the merged list is whole.
 * - SSE frames are invalidation-only; bodies are reread over RPC.
 */
import type {
  NativeActor,
  NativeChangeEvent,
  NativeCollabSnapshot,
  NativeMessage,
  NativeRuntimeBinding,
  NativeTarget,
  NativeTask,
} from '../native.ts'
import type { ChaosClient } from './api.ts'
import { CollabEvents } from './collab-events.ts'

export type CollabConnection = 'live' | 'down' | 'resyncing'

export interface CollabStoreSnapshot {
  bootstrapped: boolean
  bootstrapError: string | undefined
  selfId: string | undefined
  channels: NativeTarget[]
  actors: NativeActor[]
  activeChannelId: string | undefined
  /** Spec §1.3: the active channel vanished (membership loss) — linger, then empty. */
  removedNotice: boolean
  messagesByChannel: Record<string, NativeMessage[]>
  /** Exact server totals (history.tail.count), one per known channel. */
  totalByChannel: Record<string, number>
  /** Highest seq already pulled from the head side ('0' = none pulled yet). */
  headCursorByChannel: Record<string, string>
  /** True once the merged list covers the whole channel history. */
  headDoneByChannel: Record<string, boolean>
  unreadByChannel: Record<string, number>
  membersByChannel: Record<string, NativeActor[]>
  /** messageId → Task, for the TaskChip under message rows. */
  tasksByMessage: Record<string, NativeTask>
  /** agentId → runtime binding (only model provenance available; may be absent). */
  bindingsByAgent: Record<string, NativeRuntimeBinding>
  connection: CollabConnection
  historyLoading: boolean
  olderLoading: boolean
  historyError: string | undefined
}

const READ_PREFIX = 'dsh-chaos:read:'
const INITIAL_TAIL = 50
const HEAD_CHUNK = 100

function decimalToNumber(value: string): number {
  const parsed = Number.parseInt(value, 10)
  return Number.isFinite(parsed) ? parsed : 0
}

function compareSeq(a: string, b: string): number {
  const left = BigInt(a)
  const right = BigInt(b)
  return left < right ? -1 : left > right ? 1 : 0
}

function maxSeqOf(messages: NativeMessage[]): string | undefined {
  return messages.length === 0 ? undefined : messages[messages.length - 1]?.seq
}

interface MergeResult {
  messages: NativeMessage[]
  /** Newly arrived items appended past the previous tail (head pages never count). */
  appended: number
}

function mergeMessages(existing: NativeMessage[], incoming: NativeMessage[]): MergeResult {
  if (incoming.length === 0) return { messages: existing, appended: 0 }
  const byId = new Map(existing.map(message => [message.id, message]))
  const previousTail = maxSeqOf(existing)
  let appended = 0
  for (const message of incoming) {
    if (!byId.has(message.id)) {
      byId.set(message.id, message)
      if (previousTail === undefined || compareSeq(message.seq, previousTail) > 0) appended += 1
    }
  }
  const messages = [...byId.values()]
  messages.sort((left, right) => compareSeq(left.seq, right.seq))
  return { messages, appended }
}

function readMarker(targetId: string): number {
  try {
    const raw = localStorage.getItem(READ_PREFIX + targetId)
    if (raw === null) return 0
    const parsed = Number.parseInt(raw, 10)
    return Number.isFinite(parsed) ? parsed : 0
  } catch {
    return 0
  }
}

function writeMarker(targetId: string, total: number): void {
  try {
    localStorage.setItem(READ_PREFIX + targetId, String(total))
  } catch {
    // Storage may be unavailable (private mode); unread simply stays session-less.
  }
}

function mapByMessage(tasks: NativeTask[]): Record<string, NativeTask> {
  const mapped: Record<string, NativeTask> = {}
  for (const task of tasks) mapped[task.messageId] = task
  return mapped
}

function errorText(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

export class CollabStore {
  private snapshot: CollabStoreSnapshot = {
    bootstrapped: false,
    bootstrapError: undefined,
    selfId: undefined,
    channels: [],
    actors: [],
    activeChannelId: undefined,
    removedNotice: false,
    messagesByChannel: {},
    totalByChannel: {},
    headCursorByChannel: {},
    headDoneByChannel: {},
    unreadByChannel: {},
    membersByChannel: {},
    tasksByMessage: {},
    bindingsByAgent: {},
    connection: 'live',
    historyLoading: false,
    olderLoading: false,
    historyError: undefined,
  }

  private readonly listeners = new Set<() => void>()
  private readonly events = new CollabEvents({
    onChange: (change) => { this.handleChange(change) },
    onResyncRequired: () => { void this.resync() },
    onConnection: (state) => {
      // 'resyncing' is owned by the resync flow itself; a socket error during
      // it must not flip the banner back before the reload lands or fails.
      if (this.snapshot.connection !== 'resyncing') this.set({ connection: state })
    },
  })

  private started = false
  private loadGeneration = 0
  private removedTimer: ReturnType<typeof setTimeout> | undefined

  constructor(private readonly client: ChaosClient) {}

  readonly getSnapshot = (): CollabStoreSnapshot => this.snapshot

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  private set(patch: Partial<CollabStoreSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...patch }
    for (const listener of this.listeners) listener()
  }

  /** Idempotent entry point; also the retry path after a failed bootstrap. */
  start(): void {
    if (this.started) return
    this.started = true
    void this.bootstrap(true)
  }

  dispose(): void {
    this.loadGeneration += 1
    this.events.disconnect()
    if (this.removedTimer !== undefined) clearTimeout(this.removedTimer)
  }

  private async bootstrap(initial: boolean): Promise<void> {
    const generation = ++this.loadGeneration
    if (!initial) this.set({ bootstrapError: undefined })
    try {
      const snapshot = await this.client.snapshot()
      const [actors, tasks, bindings] = await Promise.all([
        this.client.actors(),
        this.client.tasks(),
        this.client.runtimeBindings(),
      ])
      if (this.loadGeneration !== generation) return
      const channels = snapshot.targets.filter(target => target.kind === 'channel')
      const totals = await this.seedTotals(channels, {})
      if (this.loadGeneration !== generation) return
      const bindingsByAgent: Record<string, NativeRuntimeBinding> = {}
      for (const binding of bindings) bindingsByAgent[binding.agentId] = binding
      const unread: Record<string, number> = {}
      for (const channel of channels) unread[channel.id] = Math.max(0, (totals[channel.id] ?? 0) - readMarker(channel.id))
      this.set({
        bootstrapped: true,
        bootstrapError: undefined,
        selfId: snapshot.actor.id,
        channels,
        actors,
        totalByChannel: totals,
        unreadByChannel: unread,
        tasksByMessage: mapByMessage(tasks),
        bindingsByAgent,
        historyError: undefined,
        connection: 'live',
      })
      this.events.connect(snapshot.cursor)
      const active = this.snapshot.activeChannelId
      if (active !== undefined && channels.some(channel => channel.id === active)) {
        await this.activateChannel(active, true)
      } else if (channels[0] !== undefined) {
        this.set({ activeChannelId: channels[0].id })
        await this.activateChannel(channels[0].id, true)
      }
    } catch (reason) {
      if (this.loadGeneration !== generation) return
      this.started = false
      this.set({ bootstrapError: errorText(reason) })
    }
  }

  /** tail(1) per channel without a known total; failures leave the total unknown. */
  private async seedTotals(
    channels: NativeTarget[],
    known: Record<string, number>,
  ): Promise<Record<string, number>> {
    const totals = { ...known }
    await Promise.all(channels.map(async (channel) => {
      if (totals[channel.id] !== undefined) return
      try {
        const tail = await this.client.historyTail(channel.id, 1)
        totals[channel.id] = decimalToNumber(tail.count)
      } catch {
        // One bad seed must not sink the snapshot; SSE deltas keep accumulating.
      }
    }))
    return totals
  }

  private markRead(targetId: string): void {
    const total = this.snapshot.totalByChannel[targetId] ?? 0
    writeMarker(targetId, total)
    this.set({ unreadByChannel: { ...this.snapshot.unreadByChannel, [targetId]: 0 } })
  }

  private unreadFor(targetId: string): number {
    const total = this.snapshot.totalByChannel[targetId]
    if (total === undefined) return 0
    return Math.max(0, total - readMarker(targetId))
  }

  setActiveChannel(targetId: string): void {
    if (this.snapshot.activeChannelId === targetId && !this.snapshot.removedNotice) return
    if (this.removedTimer !== undefined) {
      clearTimeout(this.removedTimer)
      this.removedTimer = undefined
    }
    this.set({
      activeChannelId: targetId,
      removedNotice: false,
      historyLoading: this.snapshot.messagesByChannel[targetId] === undefined,
      historyError: undefined,
    })
    void this.activateChannel(targetId, false)
  }

  /** (Re)load tail history + members for a channel and zero its unread marker. */
  private async activateChannel(targetId: string, force: boolean): Promise<void> {
    const generation = this.loadGeneration
    const needHistory = force || this.snapshot.messagesByChannel[targetId] === undefined
    const needMembers = force || this.snapshot.membersByChannel[targetId] === undefined
    if (needHistory) this.set({ historyLoading: true, historyError: undefined })
    try {
      const [tail, members] = await Promise.all([
        needHistory ? this.client.historyTail(targetId, INITIAL_TAIL) : Promise.resolve(undefined),
        needMembers ? this.client.targetMembers(targetId) : Promise.resolve(undefined),
      ])
      if (this.loadGeneration !== generation) return
      const patch: Partial<CollabStoreSnapshot> = {}
      let total = this.snapshot.totalByChannel[targetId]
      if (tail !== undefined) {
        const merged = mergeMessages(this.snapshot.messagesByChannel[targetId] ?? [], tail.messages)
        total = decimalToNumber(tail.count)
        patch.messagesByChannel = { ...this.snapshot.messagesByChannel, [targetId]: merged.messages }
        patch.totalByChannel = { ...this.snapshot.totalByChannel, [targetId]: total }
        patch.headDoneByChannel = {
          ...this.snapshot.headDoneByChannel,
          [targetId]: merged.messages.length >= total,
        }
        if (this.snapshot.activeChannelId === targetId) patch.historyLoading = false
      }
      if (members !== undefined) {
        patch.membersByChannel = { ...this.snapshot.membersByChannel, [targetId]: members }
      }
      if (tail !== undefined && this.snapshot.activeChannelId === targetId) {
        writeMarker(targetId, total ?? 0)
        patch.unreadByChannel = { ...this.snapshot.unreadByChannel, [targetId]: 0 }
      }
      this.set(patch)
    } catch (reason) {
      if (this.loadGeneration !== generation) return
      if (this.snapshot.activeChannelId === targetId) {
        this.set({ historyLoading: false, historyError: errorText(reason) })
      }
    }
  }

  retryActiveHistory(): void {
    const active = this.snapshot.activeChannelId
    if (active !== undefined) void this.activateChannel(active, true)
  }

  /** "Load older": one forward chunk from the head side (backend has no before cursor). */
  async loadOlder(): Promise<void> {
    const { activeChannelId, headCursorByChannel, headDoneByChannel, olderLoading } = this.snapshot
    if (activeChannelId === undefined || olderLoading) return
    if (headDoneByChannel[activeChannelId] === true) return
    const generation = this.loadGeneration
    const cursor = headCursorByChannel[activeChannelId] ?? '0'
    this.set({ olderLoading: true })
    try {
      const page = await this.client.history(activeChannelId, cursor, HEAD_CHUNK)
      if (this.loadGeneration !== generation) return
      const targetId = activeChannelId
      const merged = mergeMessages(this.snapshot.messagesByChannel[targetId] ?? [], page)
      const total = this.snapshot.totalByChannel[targetId] ?? 0
      const headSeq = maxSeqOf(page)
      this.set({
        messagesByChannel: { ...this.snapshot.messagesByChannel, [targetId]: merged.messages },
        headCursorByChannel: { ...this.snapshot.headCursorByChannel, [targetId]: headSeq ?? cursor },
        headDoneByChannel: { ...this.snapshot.headDoneByChannel, [targetId]: page.length === 0 || merged.messages.length >= total },
        olderLoading: false,
      })
    } catch {
      if (this.loadGeneration !== generation) return
      this.set({ olderLoading: false })
    }
  }

  /**
   * Send with a caller-owned requestId: retries of one attempt reuse the same
   * id, and the backend's UNIQUE(author, client_request_id) makes that safe.
   */
  async sendMessage(targetId: string, requestId: string, text: string): Promise<NativeMessage> {
    const result = await this.client.messageSend(targetId, requestId, text)
    const generation = this.loadGeneration
    if (this.loadGeneration === generation) {
      const existing = this.snapshot.messagesByChannel[targetId]
      if (existing !== undefined) {
        const merged = mergeMessages(existing, [result.message])
        const patch: Partial<CollabStoreSnapshot> = {
          messagesByChannel: { ...this.snapshot.messagesByChannel, [targetId]: merged.messages },
        }
        if (merged.appended > 0) {
          const total = (this.snapshot.totalByChannel[targetId] ?? 0) + merged.appended
          patch.totalByChannel = { ...this.snapshot.totalByChannel, [targetId]: total }
          patch.headDoneByChannel = {
            ...this.snapshot.headDoneByChannel,
            [targetId]: merged.messages.length >= total,
          }
        }
        this.set(patch)
      } else if (!result.replayed) {
        const total = (this.snapshot.totalByChannel[targetId] ?? 0) + 1
        this.set({ totalByChannel: { ...this.snapshot.totalByChannel, [targetId]: total } })
      }
      if (this.snapshot.activeChannelId === targetId) this.markRead(targetId)
    }
    return result.message
  }

  async createTask(messageId: string): Promise<NativeTask> {
    const task = await this.client.taskCreate(messageId)
    this.set({ tasksByMessage: { ...this.snapshot.tasksByMessage, [task.messageId]: task } })
    return task
  }

  async createChannel(name: string): Promise<NativeTarget> {
    const target = await this.client.channelCreate(name)
    const generation = this.loadGeneration
    if (this.loadGeneration === generation && !this.snapshot.channels.some(channel => channel.id === target.id)) {
      this.set({
        channels: [...this.snapshot.channels, target],
        totalByChannel: { ...this.snapshot.totalByChannel, [target.id]: 0 },
        unreadByChannel: { ...this.snapshot.unreadByChannel, [target.id]: 0 },
      })
      writeMarker(target.id, 0)
    }
    return target
  }

  async memberAdd(targetId: string, memberId: string): Promise<void> {
    await this.client.memberAdd(targetId, memberId)
    const members = { ...this.snapshot.membersByChannel }
    delete members[targetId]
    this.set({ membersByChannel: members })
  }

  // --- SSE invalidation handling (invalidation-only frames; bodies via RPC) ---

  private handleChange(change: NativeChangeEvent): void {
    switch (change.kind) {
      case 'message_created':
        void this.handleMessageCreated(change)
        break
      case 'target_created':
      case 'membership_changed':
        void this.reloadTargets()
        break
      case 'actor_created':
        void this.client.actors().then((actors) => { this.set({ actors }) }, () => {})
        break
      case 'task_created':
      case 'task_updated':
        void this.client.tasks().then((tasks) => {
          this.set({ tasksByMessage: mapByMessage(tasks) })
        }, () => {})
        break
      case 'thread_follow_changed':
      case 'activity_done_changed':
        // Thread surface is P0-5; activity inbox is cut. No P0-2 projection.
        break
    }
  }

  private async handleMessageCreated(change: NativeChangeEvent): Promise<void> {
    const targetId = change.targetId
    if (targetId === undefined) return
    if (!this.snapshot.channels.some(channel => channel.id === targetId)) return
    const generation = this.loadGeneration
    if (targetId === this.snapshot.activeChannelId) {
      const existing = this.snapshot.messagesByChannel[targetId]
      const afterSeq = maxSeqOf(existing ?? []) ?? '0'
      try {
        const page = await this.client.history(targetId, afterSeq, HEAD_CHUNK)
        if (this.loadGeneration !== generation) return
        const merged = mergeMessages(this.snapshot.messagesByChannel[targetId] ?? [], page)
        const patch: Partial<CollabStoreSnapshot> = {
          messagesByChannel: { ...this.snapshot.messagesByChannel, [targetId]: merged.messages },
        }
        if (merged.appended > 0) {
          const total = (this.snapshot.totalByChannel[targetId] ?? 0) + merged.appended
          patch.totalByChannel = { ...this.snapshot.totalByChannel, [targetId]: total }
          patch.headDoneByChannel = {
            ...this.snapshot.headDoneByChannel,
            [targetId]: merged.messages.length >= total,
          }
        }
        this.set(patch)
        this.markRead(targetId)
      } catch {
        // The next frame or a reconnect will re-drive the same afterSeq read.
      }
      return
    }
    // Other channels: bump the counter only, bodies stay unread (brief rule).
    const total = this.snapshot.totalByChannel[targetId]
    if (total !== undefined) {
      this.set({ totalByChannel: { ...this.snapshot.totalByChannel, [targetId]: total + 1 } })
    }
    this.set({ unreadByChannel: { ...this.snapshot.unreadByChannel, [targetId]: this.unreadFor(targetId) } })
  }

  /** target_created / membership_changed: reread the target set and seed new totals. */
  private async reloadTargets(): Promise<void> {
    const generation = this.loadGeneration
    try {
      const snapshot = await this.client.snapshot()
      if (this.loadGeneration !== generation) return
      const channels = snapshot.targets.filter(target => target.kind === 'channel')
      const totals = await this.seedTotals(channels, this.snapshot.totalByChannel)
      if (this.loadGeneration !== generation) return
      const unread: Record<string, number> = { ...this.snapshot.unreadByChannel }
      for (const channel of channels) {
        if (channel.id === this.snapshot.activeChannelId) unread[channel.id] = 0
        else if (unread[channel.id] === undefined) unread[channel.id] = Math.max(0, (totals[channel.id] ?? 0) - readMarker(channel.id))
      }
      this.set({ channels, totalByChannel: totals, unreadByChannel: unread })
      // Spec §1.3: the active channel vanished (kicked) — linger one beat with
      // an honest notice before falling back to the empty state.
      const active = this.snapshot.activeChannelId
      if (active !== undefined && !channels.some(channel => channel.id === active) && !this.snapshot.removedNotice) {
        this.set({ removedNotice: true })
        if (this.removedTimer !== undefined) clearTimeout(this.removedTimer)
        this.removedTimer = setTimeout(() => {
          this.removedTimer = undefined
          this.set({ removedNotice: false, activeChannelId: undefined })
        }, 1000)
      }
      if (active !== undefined && this.snapshot.membersByChannel[active] !== undefined) {
        void this.client.targetMembers(active).then((members) => {
          if (this.loadGeneration !== generation) return
          this.set({ membersByChannel: { ...this.snapshot.membersByChannel, [active]: members } })
        }, () => {})
      }
    } catch {
      // Snapshot failures leave the previous target set; the next frame retries.
    }
  }

  /** resync_required: cursor fell out of the retention window → full reread. */
  private async resync(): Promise<void> {
    this.set({ connection: 'resyncing', historyLoading: this.snapshot.activeChannelId !== undefined })
    this.loadGeneration += 1
    const generation = this.loadGeneration
    try {
      const snapshot: NativeCollabSnapshot = await this.client.snapshot()
      const [actors, tasks, bindings] = await Promise.all([
        this.client.actors(),
        this.client.tasks(),
        this.client.runtimeBindings(),
      ])
      if (this.loadGeneration !== generation) return
      const channels = snapshot.targets.filter(target => target.kind === 'channel')
      const totals = await this.seedTotals(channels, {})
      if (this.loadGeneration !== generation) return
      const bindingsByAgent: Record<string, NativeRuntimeBinding> = {}
      for (const binding of bindings) bindingsByAgent[binding.agentId] = binding
      let active = this.snapshot.activeChannelId
      if (active === undefined || !channels.some(channel => channel.id === active)) {
        active = channels[0]?.id
      }
      const patch: Partial<CollabStoreSnapshot> = {
        bootstrapped: true,
        selfId: snapshot.actor.id,
        channels,
        actors,
        activeChannelId: active,
        removedNotice: false,
        messagesByChannel: {},
        headCursorByChannel: {},
        headDoneByChannel: {},
        membersByChannel: {},
        totalByChannel: totals,
        tasksByMessage: mapByMessage(tasks),
        bindingsByAgent,
        historyError: undefined,
      }
      this.set(patch)
      if (active !== undefined) await this.activateChannel(active, true)
      const unread: Record<string, number> = {}
      for (const channel of channels) unread[channel.id] = this.unreadFor(channel.id)
      this.set({ unreadByChannel: unread, connection: 'live', historyLoading: false })
      this.events.connect(snapshot.cursor)
    } catch {
      if (this.loadGeneration !== generation) return
      // Stay honest: the socket is closed and the reload failed — show the
      // disconnected banner rather than looping resync frames forever.
      this.set({ connection: 'down', historyLoading: false })
    }
  }
}
