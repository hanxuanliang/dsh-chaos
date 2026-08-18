import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import type { HostObservable, InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import type { ChaosClientState } from './controller.ts'
import { resolveSentDraft } from './controller.ts'
import type { NativeMessage } from '../native.ts'
import css from './surface.module.css'

export interface ChaosInjected {
  hooks: { chaos: HostObservable<ChaosClientState> }
  ensure: () => Promise<void>
  /** Footer Activity entry: open the dock on the Activity list. */
  openActivity: () => void
  closeDock: () => void
  /** Conversation → Activity list, staying inside the dock. */
  backToList: () => void
  openDock: (targetId: string) => Promise<void>
  loadInbox: () => Promise<void>
  loadMoreInbox: () => Promise<void>
  markInboxDone: (targetId: string, throughSeq: string) => Promise<void>
  send: (text: string) => Promise<void>
  createThread: (rootMessageId: string) => Promise<void>
  openThreadPanel: (threadTargetId: string) => Promise<void>
  closeThreadPanel: () => void
  sendToThread: (text: string) => Promise<void>
  followThread: (threadTargetId: string) => Promise<void>
  unfollowThread: (threadTargetId: string) => Promise<void>
  createTask: (messageId: string) => Promise<void>
}

/** Props of every chaos surface entry (shell.overlay / sidebar.footer.action). */
export type ChaosSurfaceProps = PropsRuntime<'shell.overlay'> & InjectFace<ChaosInjected>

function useChaosStore(observable: HostObservable<ChaosClientState>): ChaosClientState {
  return useSyncExternalStore(observable.subscribe, observable.getSnapshot)
}

export function useChaos(props: InjectFace<ChaosInjected>): ChaosClientState {
  // The slot framework projects `hooks: { chaos }` into a `useChaos` hook prop;
  // fall back to direct subscription when rendering outside the framework.
  return 'useChaos' in props
    ? (props.useChaos as <T>(select: (state: ChaosClientState) => T) => T)(value => value)
    : useChaosStore((props as unknown as ChaosInjected).hooks.chaos)
}

export function authorNameOf(state: ChaosClientState, authorId: string): string {
  if (state.actor?.id === authorId) return '我'
  return state.actors.find(actor => actor.id === authorId)?.displayName ?? authorId.slice(0, 8)
}

export function timeLabel(ms: number): string {
  const date = new Date(ms)
  const now = new Date()
  const sameDay = date.toDateString() === now.toDateString()
  const hh = String(date.getHours()).padStart(2, '0')
  const mm = String(date.getMinutes()).padStart(2, '0')
  if (sameDay) return `${hh}:${mm}`
  return `${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')} ${hh}:${mm}`
}

export function relTime(ms: number): string {
  const diff = Date.now() - ms
  if (diff < 60_000) return '刚刚'
  const minutes = Math.floor(diff / 60_000)
  if (minutes < 60) return `${String(minutes)} 分钟前`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${String(hours)} 小时前`
  return timeLabel(ms)
}

interface Drafts {
  [targetId: string]: string
}

export const TASK_STATUS_TEXT: Record<string, string> = {
  todo: '待办',
  in_progress: '进行中',
  in_review: '验收中',
  done: '完成',
}

/** Message row hover actions: reply-in-thread and convert-to-task, revealed on hover/focus. */
function MessageActions(props: ChaosSurfaceProps & { state: ChaosClientState, message: NativeMessage }): React.JSX.Element {
  const { state, message } = props
  const [pending, setPending] = useState(false)
  const existingThread = state.targets.find(
    target => target.kind === 'thread' && target.rootMessageId === message.id,
  )
  const hasTask = state.tasks.some(task => task.messageId === message.id)

  const reply = async (): Promise<void> => {
    if (pending) return
    setPending(true)
    try {
      if (existingThread !== undefined) await props.openThreadPanel(existingThread.id)
      else await props.createThread(message.id)
    } finally {
      setPending(false)
    }
  }

  const convert = async (): Promise<void> => {
    if (pending || hasTask) return
    setPending(true)
    try {
      await props.createTask(message.id)
    } finally {
      setPending(false)
    }
  }

  return (
    <div className={css.msgActions}>
      <button
        type="button"
        className={css.msgActionBtn}
        aria-label="在 Thread 中回复"
        title="在 Thread 中回复"
        disabled={pending}
        onClick={() => { void reply() }}
      >
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <path d="M6.5 3.5 3 7l3.5 3.5M3 7h6a4 4 0 0 1 4 4v1.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </button>
      {!hasTask
        ? (
          <button
            type="button"
            className={css.msgActionBtn}
            aria-label="转为工作项"
            title="转为工作项"
            disabled={pending}
            onClick={() => { void convert() }}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <rect x="2.5" y="2.5" width="11" height="11" rx="2.5" stroke="currentColor" strokeWidth="1.4" />
              <path d="m5.5 8 2 2 3.5-4" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        )
        : null}
    </div>
  )
}

function ThreadPreviewRow(props: ChaosSurfaceProps & { state: ChaosClientState, message: NativeMessage }): React.JSX.Element | null {
  const { state, message } = props
  const thread = state.targets.find(
    target => target.kind === 'thread' && target.rootMessageId === message.id,
  )
  if (thread === undefined) return null
  const preview = state.threadPreviews[thread.id]
  const open = state.threadPanelId === thread.id
  return (
    <button
      type="button"
      className={css.threadPreview}
      data-open={open || undefined}
      onClick={() => { void props.openThreadPanel(thread.id) }}
    >
      <span className={css.threadPreviewCount}>
        ↳ {preview !== undefined ? `${String(preview.count)} 条回复` : '查看 Thread'}
      </span>
      {preview !== undefined && preview.latest.length > 0
        ? (
          <span className={css.threadPreviewLatest}>
            {preview.latest.map(reply => (
              <span key={reply.id} className={css.threadPreviewLine}>
                <b>{authorNameOf(state, reply.authorId)}</b>：{reply.text}
              </span>
            ))}
          </span>
        )
        : null}
    </button>
  )
}

function TaskChip(props: { state: ChaosClientState, message: NativeMessage }): React.JSX.Element | null {
  const task = props.state.tasks.find(candidate => candidate.messageId === props.message.id)
  if (task === undefined) return null
  return (
    <span className={css.taskChip} data-status={task.status}>
      <span className={css.taskChipDot} />
      工作项 #{task.number} · {TASK_STATUS_TEXT[task.status] ?? task.status}
    </span>
  )
}

export function MessageList(props: ChaosSurfaceProps & { state: ChaosClientState }): React.JSX.Element {
  const { state } = props
  const bottomRef = useRef<HTMLDivElement | null>(null)
  const count = state.messages.length
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: 'end' })
  }, [count, state.selectedTargetId])
  if (count === 0) {
    return (
      <div className={css.emptyFlow}>
        <p className={css.emptyTitle}>这里还很安静</p>
        <p className={css.emptyHint}>发出第一条消息，频道里的成员和 Agent 都能看到。</p>
      </div>
    )
  }
  let lastAuthor = ''
  let lastAt = 0
  return (
    <div className={css.flow} role="log" aria-label="消息列表">
      {state.messages.map((message: NativeMessage) => {
        const own = state.actor?.id === message.authorId
        const grouped = !own && message.authorId === lastAuthor && message.createdAtMs - lastAt < 5 * 60_000
        const showTime = message.createdAtMs - lastAt >= 5 * 60_000
        lastAuthor = message.authorId
        lastAt = message.createdAtMs
        return (
          <div key={message.id} className={css.msgWrap}>
            {showTime ? <div className={css.timeStamp}>{timeLabel(message.createdAtMs)}</div> : null}
            <div className={own ? css.msgOwn : css.msgOther} data-grouped={grouped || undefined}>
              <MessageActions {...props} message={message} />
              {!own && !grouped
                ? <div className={css.msgAuthor}>{authorNameOf(state, message.authorId)}</div>
                : null}
              <div className={own ? css.bubble : css.plainText}>{message.text}</div>
              <TaskChip state={state} message={message} />
            </div>
            <ThreadPreviewRow {...props} message={message} />
          </div>
        )
      })}
      <div ref={bottomRef} />
    </div>
  )
}

const COMPOSER_MAX_HEIGHT = 160

export function Composer(props: ChaosSurfaceProps & {
  state: ChaosClientState
  draftKey: string
  onSend: (text: string) => Promise<void>
  placeholder: string
  showMembers?: boolean
}): React.JSX.Element {
  const { state, draftKey, onSend, placeholder, showMembers = true } = props
  const [drafts, setDrafts] = useState<Drafts>({})
  const [pending, setPending] = useState(false)
  const [sendError, setSendError] = useState<string | undefined>(undefined)
  const areaRef = useRef<HTMLTextAreaElement | null>(null)
  const draft = drafts[draftKey] ?? ''

  useEffect(() => {
    const area = areaRef.current
    if (area === null) return
    area.style.height = 'auto'
    area.style.height = `${String(Math.min(area.scrollHeight, COMPOSER_MAX_HEIGHT))}px`
  }, [draft, draftKey])

  const submit = async (): Promise<void> => {
    const raw = draft
    const text = raw.trim()
    if (text === '' || pending) return
    setPending(true)
    setSendError(undefined)
    try {
      await onSend(text)
      setDrafts(current => resolveSentDraft(current, draftKey, raw) as Drafts)
    } catch (error) {
      setSendError(error instanceof Error ? error.message : String(error))
    } finally {
      setPending(false)
      areaRef.current?.focus()
    }
  }

  return (
    <div className={css.composerZone}>
      <div className={css.composerCard} data-pending={pending || undefined}>
        <textarea
          ref={areaRef}
          className={css.composerInput}
          rows={1}
          value={draft}
          placeholder={placeholder}
          aria-label="消息输入框"
          disabled={pending}
          onChange={event => {
            setDrafts(current => ({ ...current, [draftKey]: event.target.value }))
            if (sendError !== undefined) setSendError(undefined)
          }}
          onKeyDown={event => {
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault()
              void submit()
            }
          }}
        />
        <div className={css.composerBar}>
          <span className={css.composerHint}>Enter 发送 · Shift+Enter 换行</span>
          <button
            type="button"
            className={css.sendButton}
            aria-label="发送"
            disabled={pending || draft.trim() === ''}
            onClick={() => { void submit() }}
          >
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <path d="M8 12.5v-9M3.5 7 8 2.5 12.5 7" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        </div>
      </div>
      <StatusStrip state={state} sendError={sendError} showMembers={showMembers} />
    </div>
  )
}

function streamText(state: ChaosClientState): { dot: 'on' | 'busy' | 'off', text: string } {
  switch (state.stream) {
    case 'connected': return { dot: 'on', text: '实时连接' }
    case 'connecting': return { dot: 'busy', text: '连接中…' }
    case 'reconnecting': return { dot: 'busy', text: '重连中…' }
    default: return { dot: 'off', text: '未连接' }
  }
}

function StatusStrip(props: { state: ChaosClientState, sendError: string | undefined, showMembers: boolean }): React.JSX.Element {
  const { state, sendError, showMembers } = props
  const stream = streamText(state)
  return (
    <div className={css.statusStrip}>
      {sendError !== undefined
        ? <span className={css.statusError}>发送失败：{sendError}</span>
        : (
          <>
            <span className={css.statusDot} data-state={stream.dot} />
            <span>{stream.text}</span>
            {showMembers && state.members.length > 0 ? <span className={css.statusSep}>·</span> : null}
            {showMembers && state.members.length > 0 ? <span>{state.members.length} 位成员</span> : null}
          </>
        )}
    </div>
  )
}

/** Thread conversation view: follow toggle, compact flow, own composer. */
export function ThreadContextPanel(props: ChaosSurfaceProps & { state: ChaosClientState }): React.JSX.Element | null {
  const { state } = props
  const threadId = state.threadPanelId
  const [pending, setPending] = useState(false)
  const bottomRef = useRef<HTMLDivElement | null>(null)
  const count = state.threadPanelMessages.length
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: 'end' })
  }, [count, threadId])
  if (threadId === undefined) return null
  const following = state.followedThreadIds.includes(threadId)

  const toggleFollow = async (): Promise<void> => {
    if (pending) return
    setPending(true)
    try {
      if (following) await props.unfollowThread(threadId)
      else await props.followThread(threadId)
    } finally {
      setPending(false)
    }
  }

  return (
    <aside className={css.contextPanel} aria-label="Thread 面板">
      <div className={css.contextHead}>
        <span className={css.contextTitle}>Thread</span>
        <button
          type="button"
          className={css.followButton}
          data-following={following || undefined}
          disabled={pending}
          aria-pressed={following}
          onClick={() => { void toggleFollow() }}
        >
          {following ? '已关注' : '关注'}
        </button>
        <button
          type="button"
          className={css.contextClose}
          aria-label="关闭 Thread"
          onClick={() => { props.closeThreadPanel() }}
        >
          ✕
        </button>
      </div>
      <div className={css.contextFlow} role="log" aria-label="Thread 消息">
        {count === 0
          ? <p className={css.emptyHint}>还没有回复，来发第一条。</p>
          : state.threadPanelMessages.map(message => {
            const own = state.actor?.id === message.authorId
            return (
              <div key={message.id} className={own ? css.msgOwn : css.msgOther}>
                {!own ? <div className={css.msgAuthor}>{authorNameOf(state, message.authorId)}</div> : null}
                <div className={own ? css.bubble : css.plainText}>{message.text}</div>
              </div>
            )
          })}
        <div ref={bottomRef} />
      </div>
      <Composer
        {...props}
        state={state}
        draftKey={threadId}
        onSend={props.sendToThread}
        placeholder="回复 Thread…"
        showMembers={false}
      />
    </aside>
  )
}
