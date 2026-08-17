import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import type { HostObservable, InjectFace, PropsRuntime } from '@deepseek-ai/dsh-client-ui-slots'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type {} from '@deepseek-ai/dsh-client-ui-sidebar/client'
import type { ChaosClientState } from './controller.ts'
import { resolveSentDraft } from './controller.ts'
import type { NativeMessage } from '../native.ts'
import css from './Workbench.module.css'

export interface ChaosInjected {
  hooks: { chaos: HostObservable<ChaosClientState> }
  ensure: () => Promise<void>
  openWorkbench: () => void
  closeWorkbench: () => void
  selectTarget: (targetId: string) => Promise<void>
  createChannel: (name: string) => Promise<void>
  send: (text: string) => Promise<void>
}

type EntryProps = PropsRuntime<'sidebar.footer.action'> & InjectFace<ChaosInjected>
type WorkbenchProps = PropsRuntime<'shell.overlay'> & InjectFace<ChaosInjected>

function useChaosStore(observable: HostObservable<ChaosClientState>): ChaosClientState {
  return useSyncExternalStore(observable.subscribe, observable.getSnapshot)
}

function useChaos(props: WorkbenchProps): ChaosClientState {
  // The slot framework projects `hooks: { chaos }` into a `useChaos` hook prop;
  // fall back to direct subscription when rendering outside the framework.
  return 'useChaos' in props
    ? (props.useChaos as <T>(select: (state: ChaosClientState) => T) => T)(value => value)
    : useChaosStore((props as unknown as ChaosInjected).hooks.chaos)
}

function CollabGlyph(): React.JSX.Element {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="5.5" cy="5.5" r="2.5" stroke="currentColor" strokeWidth="1.4" />
      <circle cx="11" cy="10.5" r="2.5" stroke="currentColor" strokeWidth="1.4" />
      <path d="M7.6 7.6 8.9 8.9" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  )
}

/** Entry beside Settings at the sidebar foot: full row when wide, icon when rail. */
export function ChaosEntry(props: EntryProps): React.JSX.Element {
  const wide = (props as unknown as { wide?: boolean }).wide !== false
  return (
    <button
      type="button"
      className={wide ? css.entryRow : css.entryIcon}
      aria-label="协作"
      title="协作"
      onClick={() => { props.openWorkbench() }}
    >
      <CollabGlyph />
      {wide ? <span className={css.entryLabel}>协作</span> : null}
    </button>
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

function authorNameOf(state: ChaosClientState, authorId: string): string {
  if (state.actor?.id === authorId) return '我'
  return state.actors.find(actor => actor.id === authorId)?.displayName ?? authorId.slice(0, 8)
}

function timeLabel(ms: number): string {
  const date = new Date(ms)
  const now = new Date()
  const sameDay = date.toDateString() === now.toDateString()
  const hh = String(date.getHours()).padStart(2, '0')
  const mm = String(date.getMinutes()).padStart(2, '0')
  if (sameDay) return `${hh}:${mm}`
  return `${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')} ${hh}:${mm}`
}

interface Drafts {
  [targetId: string]: string
}

function MessageList(props: WorkbenchProps & { state: ChaosClientState }): React.JSX.Element {
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
          <div key={message.id}>
            {showTime ? <div className={css.timeStamp}>{timeLabel(message.createdAtMs)}</div> : null}
            <div className={own ? css.msgOwn : css.msgOther} data-grouped={grouped || undefined}>
              {!own && !grouped
                ? <div className={css.msgAuthor}>{authorNameOf(state, message.authorId)}</div>
                : null}
              <div className={own ? css.bubble : css.plainText}>{message.text}</div>
            </div>
          </div>
        )
      })}
      <div ref={bottomRef} />
    </div>
  )
}

const COMPOSER_MAX_HEIGHT = 160

function Composer(props: WorkbenchProps & { state: ChaosClientState }): React.JSX.Element {
  const { state } = props
  const targetId = state.selectedTargetId ?? ''
  const [drafts, setDrafts] = useState<Drafts>({})
  const [pending, setPending] = useState(false)
  const [sendError, setSendError] = useState<string | undefined>(undefined)
  const areaRef = useRef<HTMLTextAreaElement | null>(null)
  const draft = drafts[targetId] ?? ''

  useEffect(() => {
    const area = areaRef.current
    if (area === null) return
    area.style.height = 'auto'
    area.style.height = `${String(Math.min(area.scrollHeight, COMPOSER_MAX_HEIGHT))}px`
  }, [draft, targetId])

  const submit = async (): Promise<void> => {
    const raw = draft
    const text = raw.trim()
    if (text === '' || pending) return
    setPending(true)
    setSendError(undefined)
    try {
      await props.send(text)
      setDrafts(current => resolveSentDraft(current, targetId, raw) as Drafts)
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
          placeholder="发消息，@ 可以唤起频道里的 Agent"
          aria-label="消息输入框"
          disabled={pending}
          onChange={event => {
            setDrafts(current => ({ ...current, [targetId]: event.target.value }))
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
      <StatusStrip state={state} sendError={sendError} />
    </div>
  )
}

function StatusStrip(props: { state: ChaosClientState, sendError: string | undefined }): React.JSX.Element {
  const { state, sendError } = props
  const stream = streamText(state)
  return (
    <div className={css.statusStrip}>
      {sendError !== undefined
        ? <span className={css.statusError}>发送失败：{sendError}</span>
        : (
          <>
            <span className={css.statusDot} data-state={stream.dot} />
            <span>{stream.text}</span>
            {state.members.length > 0 ? <span className={css.statusSep}>·</span> : null}
            {state.members.length > 0 ? <span>{state.members.length} 位成员</span> : null}
          </>
        )}
    </div>
  )
}

function CreateChannelCard(props: WorkbenchProps & { onDone: () => void }): React.JSX.Element {
  const [name, setName] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | undefined>(undefined)
  const inputRef = useRef<HTMLInputElement | null>(null)
  useEffect(() => { inputRef.current?.focus() }, [])

  const submit = async (): Promise<void> => {
    const trimmed = name.trim()
    if (trimmed === '' || pending) return
    setPending(true)
    setError(undefined)
    try {
      await props.createChannel(trimmed)
      props.onDone()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
      setPending(false)
    }
  }

  return (
    <div className={css.createCard}>
      <input
        ref={inputRef}
        className={css.createInput}
        value={name}
        placeholder="频道名称"
        aria-label="频道名称"
        disabled={pending}
        onChange={event => {
          setName(event.target.value)
          if (error !== undefined) setError(undefined)
        }}
        onKeyDown={event => {
          if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
            event.preventDefault()
            void submit()
          }
          if (event.key === 'Escape') {
            event.stopPropagation()
            props.onDone()
          }
        }}
      />
      {error !== undefined ? <div className={css.createError}>{error}</div> : null}
      <div className={css.createActions}>
        <button type="button" className={css.ghostButton} disabled={pending} onClick={() => { props.onDone() }}>取消</button>
        <button
          type="button"
          className={css.primaryButton}
          disabled={pending || name.trim() === ''}
          onClick={() => { void submit() }}
        >
          {pending ? '创建中…' : '创建'}
        </button>
      </div>
    </div>
  )
}

export function Workbench(props: WorkbenchProps): React.JSX.Element | null {
  const state = useChaos(props)
  const [creating, setCreating] = useState(false)
  const plusRef = useRef<HTMLButtonElement | null>(null)
  const open = state.workbench === 'open'

  useEffect(() => {
    if (!open) return
    void props.ensure()
  }, [open]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') props.closeWorkbench()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => { window.removeEventListener('keydown', onKeyDown) }
  }, [open]) // eslint-disable-line react-hooks/exhaustive-deps

  const channels = useMemo(
    () => state.targets.filter(target => target.kind === 'channel'),
    [state.targets],
  )
  const selected = state.targets.find(target => target.id === state.selectedTargetId)

  if (!open) return null

  return (
    <div className={css.backdrop} onClick={() => { props.closeWorkbench() }}>
      <div
        className={css.panel}
        role="dialog"
        aria-modal="true"
        aria-label="协作工作台"
        onClick={event => { event.stopPropagation() }}
      >
        <aside className={css.rail}>
          <div className={css.railHead}>
            <span className={css.railTitle}>频道</span>
            <button
              ref={plusRef}
              type="button"
              className={css.plusButton}
              aria-label="新建频道"
              title="新建频道"
              onClick={() => { setCreating(true) }}
            >
              ＋
            </button>
          </div>
          {creating
            ? (
              <CreateChannelCard
                {...props}
                onDone={() => {
                  setCreating(false)
                  plusRef.current?.focus()
                }}
              />
            )
            : null}
          <div className={css.channelList}>
            {channels.map(channel => (
              <button
                key={channel.id}
                type="button"
                className={css.channelRow}
                data-active={channel.id === state.selectedTargetId || undefined}
                onClick={() => { void props.selectTarget(channel.id) }}
              >
                <span className={css.channelHash}>#</span>
                <span className={css.channelName}>{channel.name}</span>
              </button>
            ))}
            {channels.length === 0 && !creating
              ? <div className={css.railEmpty}>还没有频道，点 ＋ 创建第一个。</div>
              : null}
          </div>
        </aside>
        <main className={css.stage}>
          {selected === undefined
            ? (
              <div className={css.emptyFlow}>
                <p className={css.emptyTitle}>选择一个频道开始协作</p>
                <p className={css.emptyHint}>或者点左侧 ＋ 新建一个频道。</p>
              </div>
            )
            : (
              <>
                <header className={css.stageHead}>
                  <span className={css.stageHash}>#</span>
                  <span className={css.stageTitle}>{selected.name}</span>
                </header>
                <MessageList {...props} state={state} />
                <Composer {...props} state={state} />
              </>
            )}
        </main>
        <button
          type="button"
          className={css.closeButton}
          aria-label="关闭协作工作台"
          onClick={() => { props.closeWorkbench() }}
        >
          ✕
        </button>
      </div>
    </div>
  )
}
