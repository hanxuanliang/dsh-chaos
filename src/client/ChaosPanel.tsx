import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react'
import type { HostObservable, InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import type { ChaosClientState, ThreadPreview } from './controller.ts'
import type { NativeMessage, NativeTarget, NativeTask } from '../native.ts'
import { Avatar } from './Avatar.tsx'
import { Composer } from './Composer.tsx'
import { TaskStatusChip } from './TaskStatusChip.tsx'
import { ThreadPanel } from './ThreadPanel.tsx'
import css from './ChaosPanel.module.css'

export interface ChaosPanelInjected {
  hooks: { chaos: HostObservable<ChaosClientState> }
  ensure: () => Promise<void>
  refresh: () => Promise<void>
  togglePeek: () => void
  openWorkspace: () => void
  closeSurface: () => void
  selectTarget: (targetId: string) => Promise<void>
  createChannel: (name: string) => Promise<void>
  createDirect: (peerId: string) => Promise<void>
  addMember: (targetId: string, memberId: string) => Promise<void>
  createThread: (rootMessageId: string) => Promise<void>
  followThread: (threadTargetId: string) => Promise<void>
  unfollowThread: (threadTargetId: string) => Promise<void>
  openThreadPanel: (threadTargetId: string) => Promise<void>
  closeThreadPanel: () => void
  sendToThread: (text: string) => Promise<void>
  send: (text: string) => Promise<void>
  createTask: (messageId: string) => Promise<void>
  claimTask: (messageId: string) => Promise<void>
  unclaimTask: (task: NativeTask) => Promise<void>
  updateTask: (task: NativeTask, status: NativeTask['status']) => Promise<void>
}

export type ChaosPanelProps = PropsRuntime<'shell.overlay'> & InjectFace<ChaosPanelInjected>
export type ChaosEntryProps = PropsRuntime<'sidebar.footer.action'> & InjectFace<ChaosPanelInjected>

const taskTransitions: Record<NativeTask['status'], readonly NativeTask['status'][]> = {
  todo: ['in_progress'],
  in_progress: ['todo', 'in_review'],
  in_review: ['in_progress', 'done'],
  done: ['in_progress'],
}

const LANES: ReadonlyArray<{ status: NativeTask['status']; label: string }> = [
  { status: 'todo', label: 'TODO' },
  { status: 'in_progress', label: 'IN PROGRESS' },
  { status: 'in_review', label: 'IN REVIEW' },
  { status: 'done', label: 'DONE' },
]

const kindLabel: Record<NativeTarget['kind'], string> = {
  channel: 'CHANNEL',
  direct: 'DIRECT',
  thread: 'THREAD',
}

function timeOf(createdAtMs: number): string {
  return new Date(createdAtMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

const ReplyIcon = (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
  </svg>
)

/** Native Sidebar footer entry. The badge is authoritative pending Task count, not inferred unread state. */
export function ChaosEntry({ wide, useChaos, ensure, togglePeek }: ChaosEntryProps) {
  const state = useChaos(value => value)
  const pendingTasks = state.allTasks.filter(task => task.status !== 'done').length

  useEffect(() => { void ensure() }, [ensure])

  return (
    <div className={wide ? css.entry : `${css.entry} ${css.entryRail}`}>
      <button
        type="button"
        className={css.entryButton}
        data-active={state.surface !== 'closed' || undefined}
        aria-label="协作"
        aria-expanded={state.surface !== 'closed'}
        onClick={togglePeek}
      >
        <span className={css.entryIcon} aria-hidden>◎</span>
        {wide && <span className={css.entryLabel}>协作</span>}
        {pendingTasks > 0 && <span className={css.entryCount}>{pendingTasks}</span>}
      </button>
    </div>
  )
}

function QuickPeek({
  state,
  closeSurface,
  openWorkspace,
  selectTarget,
}: Pick<ChaosPanelProps, 'closeSurface' | 'openWorkspace' | 'selectTarget'> & { state: ChaosClientState }) {
  const channels = state.targets.filter(target => target.kind === 'channel').slice(0, 8)
  const pendingTasks = state.allTasks.filter(task => task.status !== 'done').length
  const openTarget = (targetId: string): void => {
    void selectTarget(targetId).then(openWorkspace)
  }

  return (
    <aside className={css.peek} aria-label="协作速览">
      <header className={css.peekHeader}>
        <div>
          <strong>协作</strong>
          <small>{state.stream === 'connected' ? '实时连接' : state.stream === 'reconnecting' ? '重连中…' : state.stream}</small>
        </div>
        <button type="button" className={css.iconButton} aria-label="关闭协作速览" onClick={closeSurface}>×</button>
      </header>
      <div className={css.peekSummary}>
        <span>{channels.length} 个 Channel</span>
        <span>{pendingTasks} 个待处理 Task</span>
      </div>
      <div className={css.peekTargets}>
        {channels.length === 0 && <p className={css.empty}>还没有 Channel。</p>}
        {channels.map(target => (
          <button
            type="button"
            key={target.id}
            className={css.peekTarget}
            aria-label={`打开 Channel ${target.name}`}
            onClick={() => { openTarget(target.id) }}
          >
            <span className={css.targetGlyph} aria-hidden>#</span>
            <span>{target.name}</span>
          </button>
        ))}
      </div>
      <button type="button" className={css.primaryButton} aria-label="打开协作工作台" onClick={openWorkspace}>
        打开协作工作台
      </button>
    </aside>
  )
}

/** Centered lightweight Channel creation dialog (replaces the inline nav form). */
function CreateChannelDialog({
  pending,
  error,
  onCancel,
  onCreate,
}: {
  pending: boolean
  error: string | null
  onCancel: () => void
  onCreate: (name: string) => Promise<boolean>
}) {
  const [name, setName] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => { inputRef.current?.focus() }, [])
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        onCancel()
      }
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => { window.removeEventListener('keydown', onKeyDown, true) }
  }, [onCancel])

  const submit = (event: FormEvent): void => {
    event.preventDefault()
    const trimmed = name.trim()
    if (trimmed === '' || pending) return
    void onCreate(trimmed).then(created => {
      if (created) setName('')
      else inputRef.current?.focus()
    })
  }

  return (
    <div
      className={css.dialogOverlay}
      onMouseDown={event => {
        if (event.target === event.currentTarget) onCancel()
      }}
    >
      <form className={css.dialog} aria-label="新建 Channel" onSubmit={submit}>
        <strong className={css.dialogTitle}>新建 Channel</strong>
        <input
          ref={inputRef}
          value={name}
          onChange={event => { setName(event.target.value) }}
          placeholder="Channel 名称"
          aria-label="Channel 名称"
        />
        {error !== null && <div className={css.composerError} role="alert">{error}</div>}
        <div className={css.dialogActions}>
          <button type="button" className={css.secondaryButton} onClick={onCancel}>取消</button>
          <button className={css.primaryButton} disabled={pending || name.trim() === ''}>
            {pending ? '创建中…' : '创建'}
          </button>
        </div>
      </form>
    </div>
  )
}

/** One chat-timeline row: no card chrome; the reply action appears on hover/focus. */
function MessageRow({
  message,
  showHeader,
  names,
  kinds,
  thread,
  preview,
  pending,
  onReply,
  onOpenThread,
  registerTrigger,
}: {
  message: NativeMessage
  showHeader: boolean
  names: ReadonlyMap<string, string>
  kinds: ReadonlyMap<string, string>
  thread: NativeTarget | undefined
  preview: ThreadPreview | undefined
  pending: boolean
  onReply: (message: NativeMessage, trigger: HTMLElement) => void
  onOpenThread: (threadTargetId: string, trigger: HTMLElement) => void
  registerTrigger: (trigger: HTMLElement) => void
}) {
  return (
    <div className={css.messageRow} data-has-header={showHeader || undefined}>
      {showHeader && (
        <div className={css.messageRowHeader}>
          <Avatar seed={message.authorId} size={22} />
          <strong>{names.get(message.authorId) ?? message.authorId}</strong>
          {kinds.get(message.authorId) === 'agent' && <span className={css.badge}>agent</span>}
          <time>{timeOf(message.createdAtMs)}</time>
        </div>
      )}
      <p className={css.messageText}>{message.text}</p>
      {thread !== undefined && (
        <button
          type="button"
          className={css.threadPreview}
          disabled={pending}
          onClick={event => {
            registerTrigger(event.currentTarget)
            onOpenThread(thread.id, event.currentTarget)
          }}
        >
          <span className={css.threadPreviewHead}>
            <b>{preview === undefined ? '…' : preview.count === 0 ? '回复' : `${String(preview.count)} 条回复`}</b> ›
          </span>
          {preview?.latest.map(reply => (
            <span key={reply.id} className={css.threadPreviewRow}>
              <b>{names.get(reply.authorId) ?? reply.authorId}</b>
              {kinds.get(reply.authorId) === 'agent' && <span className={css.badge}>agent</span>}
              <span className={css.threadPreviewSnippet}>{reply.text}</span>
              <time>{timeOf(reply.createdAtMs)}</time>
            </span>
          ))}
        </button>
      )}
      <div className={css.hoverActions}>
        <button
          type="button"
          aria-label="回复"
          title="回复"
          disabled={pending}
          onClick={event => {
            registerTrigger(event.currentTarget)
            onReply(message, event.currentTarget)
          }}
        >
          {ReplyIcon}
        </button>
      </div>
    </div>
  )
}

/** Channel-top task board: four swim lanes; status moves via the chip menu. */
function TaskBoard({
  tasks,
  textByMessage,
  names,
  actorId,
  channelCreatorId,
  pending,
  onUpdate,
  onClaim,
  onUnclaim,
}: {
  tasks: readonly NativeTask[]
  textByMessage: ReadonlyMap<string, string>
  names: ReadonlyMap<string, string>
  actorId: string | undefined
  channelCreatorId: string | undefined
  pending: boolean
  onUpdate: (task: NativeTask, status: NativeTask['status']) => void
  onClaim: (task: NativeTask) => void
  onUnclaim: (task: NativeTask) => void
}) {
  return (
    <div className={css.board}>
      {LANES.map(lane => {
        const laneTasks = tasks.filter(task => task.status === lane.status)
        return (
          <section key={lane.status} className={css.lane} aria-label={lane.label}>
            <h4>
              <span className={css.laneTitle}>
                <span className={css.laneDot} data-status={lane.status} aria-hidden />
                {lane.label}
              </span>
              <span className={css.laneCount}>{laneTasks.length}</span>
            </h4>
            {laneTasks.map(task => {
              const title = textByMessage.get(task.messageId)
              return (
                <article key={task.messageId} className={css.taskCard}>
                  <header>
                    <strong>#{task.number}</strong>
                    <TaskStatusChip
                      task={task}
                      transitions={taskTransitions[task.status]}
                      canUpdate={
                        task.assigneeId === undefined
                        || task.assigneeId === actorId
                        || channelCreatorId === actorId
                      }
                      pending={pending}
                      onUpdate={status => { onUpdate(task, status) }}
                    />
                  </header>
                  {title !== undefined && <p className={css.taskCardTitle}>{title}</p>}
                  <div className={css.taskCardWho}>
                    {task.assigneeId === undefined
                      ? '未认领'
                      : (
                        <>
                          <Avatar seed={task.assigneeId} size={16} />
                          {names.get(task.assigneeId) ?? task.assigneeId}
                        </>
                      )}
                  </div>
                  <div className={css.taskCardActions}>
                    {task.assigneeId === undefined && task.status !== 'done' && (
                      <button type="button" disabled={pending} onClick={() => { onClaim(task) }}>认领</button>
                    )}
                    {task.assigneeId === actorId && task.status !== 'done' && (
                      <button type="button" disabled={pending} onClick={() => { onUnclaim(task) }}>取消认领</button>
                    )}
                  </div>
                </article>
              )
            })}
          </section>
        )
      })}
    </div>
  )
}

/** Frame-wide collaboration workspace and its lighter Quick Peek state. */
export function ChaosPanel({
  useChaos,
  ensure,
  openWorkspace,
  closeSurface,
  selectTarget,
  createChannel,
  addMember,
  createThread,
  openThreadPanel,
  closeThreadPanel,
  sendToThread,
  send,
  claimTask,
  unclaimTask,
  updateTask,
}: ChaosPanelProps) {
  const state = useChaos(value => value)
  const [createOpen, setCreateOpen] = useState(false)
  const [createPending, setCreatePending] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)
  const [drafts, setDrafts] = useState<Readonly<Record<string, string>>>({})
  const [sendPending, setSendPending] = useState(false)
  const [sendError, setSendError] = useState<string | null>(null)
  const [threadSendPending, setThreadSendPending] = useState(false)
  const [threadSendError, setThreadSendError] = useState<string | null>(null)
  const [memberId, setMemberId] = useState('')
  const [pending, setPending] = useState(false)
  const [detailsOpen, setDetailsOpen] = useState(false)
  const [view, setView] = useState<'messages' | 'tasks'>('messages')
  const [failure, setFailure] = useState<string | null>(null)
  const plusRef = useRef<HTMLButtonElement>(null)
  const threadTriggerRef = useRef<HTMLElement | null>(null)

  const names = useMemo(
    () => new Map(state.actors.map(actor => [actor.id, actor.displayName])),
    [state.actors],
  )
  const kinds = useMemo(
    () => new Map(state.actors.map(actor => [actor.id, actor.kind])),
    [state.actors],
  )
  const selected = state.targets.find(target => target.id === state.selectedTargetId)
  const textByMessage = useMemo(
    () => new Map(state.messages.map(message => [message.id, message.text])),
    [state.messages],
  )
  const threadByRoot = useMemo(
    () => new Map(
      state.targets
        .filter(target => target.kind === 'thread' && target.rootMessageId !== undefined)
        .map(target => [target.rootMessageId as string, target]),
    ),
    [state.targets],
  )
  const channels = state.targets.filter(target => target.kind === 'channel')
  const panelThread = state.targets.find(target => target.id === state.threadPanelId)
  const panelParent = panelThread === undefined
    ? undefined
    : state.targets.find(target => target.id === panelThread.parentTargetId)

  useEffect(() => { void ensure() }, [ensure])
  useEffect(() => {
    if (state.surface !== 'workspace') setDetailsOpen(false)
  }, [state.surface])
  // An open Thread panel owns the right rail; on narrow screens the rail is a
  // drawer, so opening a thread must open the drawer too.
  useEffect(() => {
    if (state.threadPanelId !== undefined) setDetailsOpen(true)
  }, [state.threadPanelId])
  // Reset the channel view when switching targets.
  useEffect(() => { setView('messages') }, [state.selectedTargetId])
  useEffect(() => {
    if (state.surface === 'closed') return
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== 'Escape') return
      if (createOpen) return // The dialog handles its own Escape.
      if (detailsOpen) closeDetails()
      else closeSurface()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => { window.removeEventListener('keydown', onKeyDown) }
  })

  const run = async (operation: () => Promise<void>): Promise<void> => {
    setPending(true)
    setFailure(null)
    try {
      await operation()
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error))
    } finally {
      setPending(false)
    }
  }

  const closeDetails = (): void => {
    setDetailsOpen(false)
    threadTriggerRef.current?.focus()
  }

  const draftKey = state.selectedTargetId ?? ''
  const draft = drafts[draftKey] ?? ''
  const setDraft = (value: string): void => {
    setDrafts(previous => ({ ...previous, [draftKey]: value }))
  }
  const threadDraftKey = state.threadPanelId ?? ''
  const threadDraft = drafts[threadDraftKey] ?? ''
  const setThreadDraft = (value: string): void => {
    setDrafts(previous => ({ ...previous, [threadDraftKey]: value }))
  }

  const submitMessage = async (): Promise<void> => {
    const text = draft.trim()
    if (text === '' || sendPending) return
    setSendPending(true)
    setSendError(null)
    try {
      await send(text)
      setDraft('')
    } catch (error) {
      setSendError(error instanceof Error ? error.message : String(error))
    } finally {
      setSendPending(false)
    }
  }

  const submitThreadMessage = async (): Promise<void> => {
    const text = threadDraft.trim()
    if (text === '' || threadSendPending) return
    setThreadSendPending(true)
    setThreadSendError(null)
    try {
      await sendToThread(text)
      setThreadDraft('')
    } catch (error) {
      setThreadSendError(error instanceof Error ? error.message : String(error))
    } finally {
      setThreadSendPending(false)
    }
  }

  const submitCreateChannel = async (name: string): Promise<boolean> => {
    if (createPending) return false
    setCreatePending(true)
    setCreateError(null)
    try {
      await createChannel(name)
      setCreateOpen(false)
      plusRef.current?.focus()
      return true
    } catch (error) {
      // Failure keeps the input so the name is not lost.
      setCreateError(error instanceof Error ? error.message : String(error))
      return false
    } finally {
      setCreatePending(false)
    }
  }

  const submitMember = (event: FormEvent): void => {
    event.preventDefault()
    if (selected?.kind !== 'channel' || memberId === '') return
    void run(async () => {
      await addMember(selected.id, memberId)
      setMemberId('')
    })
  }

  const openThreadFrom = (threadTargetId: string): void => {
    void run(() => openThreadPanel(threadTargetId))
  }

  const replyTo = (message: NativeMessage): void => {
    const thread = threadByRoot.get(message.id)
    if (thread !== undefined) openThreadFrom(thread.id)
    else void run(() => createThread(message.id))
  }

  if (state.surface === 'closed') return null
  if (state.surface === 'peek') {
    return (
      <QuickPeek
        state={state}
        closeSurface={closeSurface}
        openWorkspace={openWorkspace}
        selectTarget={selectTarget}
      />
    )
  }

  return (
    <div className={css.backdrop}>
      <section className={css.workspace} aria-label="协作工作台">
        <header className={css.workspaceHeader}>
          <div className={css.workspaceTitle}>
            <span className={css.brandIcon} aria-hidden>◎</span>
            <strong>协作工作台</strong>
            <small>
              {state.actor?.displayName ?? '本地用户'}
              {' · '}
              {state.stream === 'connected' ? '实时连接' : '重连中…'}
            </small>
          </div>
          <button type="button" className={css.iconButton} aria-label="关闭协作工作台" onClick={closeSurface}>×</button>
        </header>

        <div className={css.workspaceBody}>
          <aside className={css.targetNav}>
            <div className={css.navHead}>
              <h3>CHANNELS</h3>
              <button
                ref={plusRef}
                type="button"
                className={css.plusButton}
                aria-label="新建 Channel"
                title="新建 Channel"
                onClick={() => {
                  setCreateError(null)
                  setCreateOpen(true)
                }}
              >
                ＋
              </button>
            </div>
            <nav className={css.targets} aria-label="Channels">
              {channels.map(channel => (
                <button
                  type="button"
                  key={channel.id}
                  className={css.targetButton}
                  data-selected={channel.id === state.selectedTargetId || undefined}
                  aria-label={`打开 Channel ${channel.name}`}
                  onClick={() => { void selectTarget(channel.id) }}
                >
                  <span className={css.targetGlyph} aria-hidden>#</span>
                  <span>{channel.name}</span>
                </button>
              ))}
              {channels.length === 0 && <p className={css.empty}>用 ＋ 新建一个 Channel 开始协作。</p>}
            </nav>
          </aside>

          <main className={css.conversation}>
            <header className={css.conversationHeader}>
              <div>
                <span className={css.targetKind}>{selected === undefined ? 'CHANNEL' : kindLabel[selected.kind]}</span>
                <h2>{selected === undefined ? '请选择 Channel' : selected.name}</h2>
              </div>
              <button type="button" className={css.mobileDetailsButton} onClick={() => { setDetailsOpen(true) }}>
                面板
              </button>
            </header>
            <div className={css.viewTabs} role="tablist">
              <button
                type="button"
                role="tab"
                aria-selected={view === 'messages'}
                data-active={view === 'messages' || undefined}
                onClick={() => { setView('messages') }}
              >
                消息
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={view === 'tasks'}
                data-active={view === 'tasks' || undefined}
                onClick={() => { setView('tasks') }}
              >
                任务{state.tasks.length > 0 ? ` ${String(state.tasks.length)}` : ''}
              </button>
            </div>
            {(state.error !== undefined || failure !== null) && (
              <div className={css.error} role="alert">{failure ?? state.error}</div>
            )}
            {view === 'tasks' ? (
              <TaskBoard
                tasks={state.tasks}
                textByMessage={textByMessage}
                names={names}
                actorId={state.actor?.id}
                channelCreatorId={selected?.createdBy}
                pending={pending}
                onUpdate={(task, status) => { void run(() => updateTask(task, status)) }}
                onClaim={task => { void run(() => claimTask(task.messageId)) }}
                onUnclaim={task => { void run(() => unclaimTask(task)) }}
              />
            ) : (
              <>
                <div className={css.messages}>
                  {state.status === 'loading' && <p className={css.empty}>正在加载……</p>}
                  {selected !== undefined && state.messages.length === 0 && state.status === 'ready' && (
                    <p className={css.empty}>这里还没有消息。</p>
                  )}
                  {state.messages.map((message, index) => {
                    const previous = index > 0 ? state.messages[index - 1] : undefined
                    const thread = threadByRoot.get(message.id)
                    return (
                      <MessageRow
                        key={message.id}
                        message={message}
                        showHeader={previous === undefined || previous.authorId !== message.authorId}
                        names={names}
                        kinds={kinds}
                        thread={thread}
                        preview={thread === undefined ? undefined : state.threadPreviews[thread.id]}
                        pending={pending}
                        onReply={replyTo}
                        onOpenThread={openThreadFrom}
                        registerTrigger={trigger => { threadTriggerRef.current = trigger }}
                      />
                    )
                  })}
                </div>
                <Composer
                  value={draft}
                  onChange={setDraft}
                  onSend={submitMessage}
                  pending={sendPending}
                  error={sendError}
                  placeholder={selected === undefined ? '发送消息' : `发送到 #${selected.name}`}
                  ariaLabel="发送消息"
                />
              </>
            )}
          </main>

          <aside className={css.details} data-open={detailsOpen || undefined}>
            {panelThread !== undefined ? (
              <ThreadPanel
                thread={panelThread}
                parentName={panelParent?.name ?? 'Thread'}
                messages={state.threadPanelMessages}
                names={names}
                kinds={kinds}
                draft={threadDraft}
                onDraftChange={setThreadDraft}
                pending={threadSendPending}
                error={threadSendError}
                onClose={() => {
                  closeThreadPanel()
                  closeDetails()
                }}
                onSend={submitThreadMessage}
              />
            ) : (
              <div className={css.membersPane}>
                <div className={css.membersHead}>
                  <h3>成员 {state.actors.length}</h3>
                  <button
                    type="button"
                    className={`${css.iconButton} ${css.railClose}`}
                    aria-label="关闭面板"
                    onClick={closeDetails}
                  >
                    ×
                  </button>
                </div>
                {state.actors.map(actor => (
                  <div key={actor.id} className={css.memberRow}>
                    <Avatar seed={actor.id} size={18} />
                    <span className={css.memberName}>
                      {actor.displayName}{actor.id === state.actor?.id ? '（你）' : ''}
                    </span>
                    <span className={css.badge} data-kind={actor.kind}>{actor.kind}</span>
                  </div>
                ))}
                {selected?.kind === 'channel' && (
                  <form onSubmit={submitMember} className={css.memberForm}>
                    <select
                      value={memberId}
                      onChange={event => { setMemberId(event.target.value) }}
                      aria-label="邀请成员或 agent"
                    >
                      <option value="">邀请成员或 agent…</option>
                      {state.actors.filter(actor => actor.id !== state.actor?.id).map(actor => (
                        <option key={actor.id} value={actor.id}>{actor.displayName} (@{actor.handle})</option>
                      ))}
                    </select>
                    <button className={css.secondaryButton} disabled={pending || memberId === ''}>加入</button>
                  </form>
                )}
              </div>
            )}
          </aside>
        </div>
      </section>
      {createOpen && (
        <CreateChannelDialog
          pending={createPending}
          error={createError}
          onCancel={() => {
            setCreateOpen(false)
            plusRef.current?.focus()
          }}
          onCreate={submitCreateChannel}
        />
      )}
    </div>
  )
}
