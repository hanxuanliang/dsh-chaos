import { useEffect, useMemo, useState, type FormEvent } from 'react'
import type { HostObservable, InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import type { ChaosClientState } from './controller.ts'
import type { NativeTarget, NativeTask } from '../native.ts'
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

const kindLabel: Record<NativeTarget['kind'], string> = {
  channel: 'Channel',
  direct: 'Direct',
  thread: 'Thread',
}

function visibleTargets(state: ChaosClientState): readonly NativeTarget[] {
  const followed = new Set(state.followedThreadIds)
  return state.targets
    .filter(target => target.kind !== 'thread' || followed.has(target.id))
    .toSorted((left, right) => right.createdAtMs - left.createdAtMs)
}

function targetDisplayName(target: NativeTarget, allTargets: readonly NativeTarget[]): string {
  if (target.kind !== 'thread') return target.name
  const parent = allTargets.find(candidate => candidate.id === target.parentTargetId)
  const rootSuffix = target.rootMessageId?.slice(-5) ?? target.id.slice(-5)
  return `${parent?.name ?? 'Thread'} · ${rootSuffix}`
}

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
  const targets = visibleTargets(state).slice(0, 8)
  const pendingTasks = state.allTasks.filter(task => task.status !== 'done').length
  const openTarget = (targetId: string): void => {
    void selectTarget(targetId).then(openWorkspace)
  }

  return (
    <aside className={css.peek} aria-label="协作速览">
      <header className={css.peekHeader}>
        <div>
          <strong>协作</strong>
          <small>{state.stream === 'connected' ? '实时连接' : state.stream}</small>
        </div>
        <button type="button" className={css.iconButton} aria-label="关闭协作速览" onClick={closeSurface}>×</button>
      </header>
      <div className={css.peekSummary}>
        <span>{targets.length} 个目标</span>
        <span>{pendingTasks} 个待处理 Task</span>
      </div>
      <div className={css.peekTargets}>
        {targets.length === 0 && <p className={css.empty}>还没有 Channel 或 Direct。</p>}
        {targets.map(target => (
          <button
            type="button"
            key={target.id}
            className={css.peekTarget}
            aria-label={`打开 ${kindLabel[target.kind]} ${targetDisplayName(target, state.targets)}`}
            onClick={() => { openTarget(target.id) }}
          >
            <span className={css.targetKind}>{kindLabel[target.kind]}</span>
            <span>{targetDisplayName(target, state.targets)}</span>
          </button>
        ))}
      </div>
      <button type="button" className={css.primaryButton} aria-label="打开协作工作台" onClick={openWorkspace}>
        打开协作工作台
      </button>
    </aside>
  )
}

function TargetGroup({
  title,
  targets,
  selectedTargetId,
  selectTarget,
  allTargets,
}: {
  title: string
  targets: readonly NativeTarget[]
  selectedTargetId: string | undefined
  selectTarget: (targetId: string) => Promise<void>
  allTargets: readonly NativeTarget[]
}) {
  if (targets.length === 0) return null
  return (
    <section className={css.targetGroup}>
      <h3>{title}</h3>
      {targets.map(target => (
        <button
          type="button"
          key={target.id}
          className={css.targetButton}
          data-selected={target.id === selectedTargetId || undefined}
          aria-label={`打开 ${kindLabel[target.kind]} ${targetDisplayName(target, allTargets)}`}
          onClick={() => { void selectTarget(target.id) }}
        >
          <span className={css.targetGlyph} aria-hidden>{target.kind === 'channel' ? '#' : target.kind === 'direct' ? '@' : '↳'}</span>
          <span>{targetDisplayName(target, allTargets)}</span>
        </button>
      ))}
    </section>
  )
}

/** Frame-wide collaboration workspace and its lighter Quick Peek state. */
export function ChaosPanel({
  useChaos,
  ensure,
  refresh,
  openWorkspace,
  closeSurface,
  selectTarget,
  createChannel,
  createDirect,
  addMember,
  createThread,
  followThread,
  unfollowThread,
  send,
  createTask,
  claimTask,
  unclaimTask,
  updateTask,
}: ChaosPanelProps) {
  const state = useChaos(value => value)
  const [channelName, setChannelName] = useState('')
  const [draft, setDraft] = useState('')
  const [peerId, setPeerId] = useState('')
  const [memberId, setMemberId] = useState('')
  const [pending, setPending] = useState(false)
  const [detailsOpen, setDetailsOpen] = useState(false)
  const [failure, setFailure] = useState<string | null>(null)
  const names = useMemo(
    () => new Map(state.actors.map(actor => [actor.id, actor.displayName])),
    [state.actors],
  )
  const followed = useMemo(() => new Set(state.followedThreadIds), [state.followedThreadIds])
  const selected = state.targets.find(target => target.id === state.selectedTargetId)
  const taskByMessage = useMemo(
    () => new Map(state.tasks.map(task => [task.messageId, task])),
    [state.tasks],
  )
  const channels = state.targets.filter(target => target.kind === 'channel')
  const directs = state.targets.filter(target => target.kind === 'direct')
  const threads = state.targets.filter(target => target.kind === 'thread' && followed.has(target.id))

  useEffect(() => { void ensure() }, [ensure])
  useEffect(() => {
    if (state.surface !== 'workspace') setDetailsOpen(false)
  }, [state.surface])
  useEffect(() => {
    if (state.surface === 'closed') return
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== 'Escape') return
      if (detailsOpen) setDetailsOpen(false)
      else closeSurface()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => { window.removeEventListener('keydown', onKeyDown) }
  }, [closeSurface, detailsOpen, state.surface])

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

  const submitChannel = (event: FormEvent): void => {
    event.preventDefault()
    const name = channelName.trim()
    if (name === '') return
    void run(async () => {
      await createChannel(name)
      setChannelName('')
    })
  }

  const submitDirect = (event: FormEvent): void => {
    event.preventDefault()
    if (peerId === '') return
    void run(async () => {
      await createDirect(peerId)
      setPeerId('')
    })
  }

  const submitMember = (event: FormEvent): void => {
    event.preventDefault()
    if (selected?.kind !== 'channel' || memberId === '') return
    void run(async () => {
      await addMember(selected.id, memberId)
      setMemberId('')
    })
  }

  const submitMessage = (event: FormEvent): void => {
    event.preventDefault()
    const text = draft.trim()
    if (text === '') return
    void run(async () => {
      await send(text)
      setDraft('')
    })
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
            <div>
              <strong>协作工作台</strong>
              <small>{state.actor?.displayName ?? '本地用户'} · {state.stream === 'connected' ? '实时连接' : state.stream}</small>
            </div>
          </div>
          <div className={css.headerActions}>
            <button type="button" className={css.secondaryButton} disabled={pending} onClick={() => { void refresh() }}>刷新</button>
            <button type="button" className={css.iconButton} aria-label="关闭协作工作台" onClick={closeSurface}>×</button>
          </div>
        </header>

        <div className={css.workspaceBody}>
          <aside className={css.targetNav}>
            <form onSubmit={submitChannel} className={css.compactForm}>
              <input value={channelName} onChange={event => { setChannelName(event.target.value) }} placeholder="新建 Channel" />
              <button disabled={pending || channelName.trim() === ''}>新建</button>
            </form>
            <form onSubmit={submitDirect} className={css.compactForm}>
              <select value={peerId} onChange={event => { setPeerId(event.target.value) }}>
                <option value="">选择直聊对象</option>
                {state.actors.filter(actor => actor.id !== state.actor?.id).map(actor => (
                  <option key={actor.id} value={actor.id}>{actor.displayName} (@{actor.handle})</option>
                ))}
              </select>
              <button disabled={pending || peerId === ''}>直聊</button>
            </form>
            <nav className={css.targets} aria-label="协作目标">
              <TargetGroup title="Channels" targets={channels} selectedTargetId={state.selectedTargetId} selectTarget={selectTarget} allTargets={state.targets} />
              <TargetGroup title="Direct" targets={directs} selectedTargetId={state.selectedTargetId} selectTarget={selectTarget} allTargets={state.targets} />
              <TargetGroup title="Followed Threads" targets={threads} selectedTargetId={state.selectedTargetId} selectTarget={selectTarget} allTargets={state.targets} />
              {state.targets.length === 0 && <p className={css.empty}>新建一个 Channel 开始协作。</p>}
            </nav>
          </aside>

          <main className={css.conversation}>
            <header className={css.conversationHeader}>
              <div>
                <span className={css.targetKind}>{selected === undefined ? '协作目标' : kindLabel[selected.kind]}</span>
                <h2>{selected === undefined ? '请选择协作目标' : targetDisplayName(selected, state.targets)}</h2>
              </div>
              <div className={css.headerActions}>
                {selected?.kind === 'thread' && (
                  <button
                    type="button"
                    className={followed.has(selected.id) ? css.secondaryButton : css.primaryButton}
                    disabled={pending}
                    onClick={() => { void run(() => followed.has(selected.id) ? unfollowThread(selected.id) : followThread(selected.id)) }}
                  >
                    {followed.has(selected.id) ? '取消关注' : '关注 Thread'}
                  </button>
                )}
                <button type="button" className={css.mobileDetailsButton} onClick={() => { setDetailsOpen(true) }}>
                  工作项{state.tasks.length > 0 ? ` ${String(state.tasks.length)}` : ''}
                </button>
              </div>
            </header>
            {(state.error !== undefined || failure !== null) && (
              <div className={css.error} role="alert">{failure ?? state.error}</div>
            )}
            <div className={css.messages}>
              {state.status === 'loading' && <p className={css.empty}>正在加载……</p>}
              {selected !== undefined && state.messages.length === 0 && <p className={css.empty}>这里还没有消息。</p>}
              {state.messages.map(message => (
                <article key={message.id} className={css.message}>
                  <header>
                    <strong>{names.get(message.authorId) ?? message.authorId}</strong>
                    <time>{new Date(message.createdAtMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
                  </header>
                  <p>{message.text}</p>
                  {selected?.kind !== 'thread' && (
                    <footer>
                      <button type="button" disabled={pending} onClick={() => { void run(() => createThread(message.id)) }}>打开 Thread</button>
                      {!taskByMessage.has(message.id) && (
                        <button type="button" disabled={pending} onClick={() => { void run(() => createTask(message.id)) }}>创建 Task</button>
                      )}
                    </footer>
                  )}
                </article>
              ))}
            </div>
            <form onSubmit={submitMessage} className={css.composer}>
              <input
                value={draft}
                onChange={event => { setDraft(event.target.value) }}
                placeholder="发送消息"
                disabled={selected === undefined || pending}
              />
              <button className={css.primaryButton} disabled={selected === undefined || pending || draft.trim() === ''}>发送</button>
            </form>
          </main>

          <aside className={css.details} data-open={detailsOpen || undefined}>
            <div className={css.mobileDetailsHeader}>
              <strong>工作项与成员</strong>
              <button type="button" className={css.iconButton} aria-label="关闭工作项" onClick={() => { setDetailsOpen(false) }}>×</button>
            </div>
            <section>
              <h3>Tasks</h3>
              {state.tasks.length === 0 && <p className={css.empty}>当前目标没有 Task。</p>}
              <div className={css.taskList}>
                {state.tasks.map(task => (
                  <article key={task.messageId} className={css.task}>
                    <header><strong>#{task.number}</strong><span data-status={task.status}>{task.status}</span></header>
                    <p>{task.assigneeId === undefined ? '未认领' : names.get(task.assigneeId) ?? task.assigneeId}</p>
                    <div className={css.taskActions}>
                      {task.assigneeId === undefined && task.status !== 'done' && (
                        <button type="button" disabled={pending} onClick={() => { void run(() => claimTask(task.messageId)) }}>认领</button>
                      )}
                      {task.assigneeId === state.actor?.id && task.status !== 'done' && (
                        <button type="button" disabled={pending} onClick={() => { void run(() => unclaimTask(task)) }}>取消认领</button>
                      )}
                      {(task.assigneeId === undefined || task.assigneeId === state.actor?.id || selected?.createdBy === state.actor?.id)
                        && taskTransitions[task.status].map(status => (
                          <button type="button" key={status} disabled={pending} onClick={() => { void run(() => updateTask(task, status)) }}>{status}</button>
                        ))}
                    </div>
                  </article>
                ))}
              </div>
            </section>
            {selected?.kind === 'channel' && (
              <section>
                <h3>Channel 成员</h3>
                <form onSubmit={submitMember} className={css.memberForm}>
                  <select value={memberId} onChange={event => { setMemberId(event.target.value) }}>
                    <option value="">选择要加入的成员</option>
                    {state.actors.filter(actor => actor.id !== state.actor?.id).map(actor => (
                      <option key={actor.id} value={actor.id}>{actor.displayName} (@{actor.handle})</option>
                    ))}
                  </select>
                  <button disabled={pending || memberId === ''}>加入</button>
                </form>
              </section>
            )}
          </aside>
        </div>
      </section>
    </div>
  )
}
