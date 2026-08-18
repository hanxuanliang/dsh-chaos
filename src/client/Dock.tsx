import { useEffect, useRef, useState } from 'react'
import type {} from '@deepseek-ai/dsh-client-ui-layout/client'
import type { NativeActivityInboxItem } from '../native.ts'
import type { ChaosClientState } from './controller.ts'
import {
  Composer,
  MessageList,
  relTime,
  TASK_STATUS_TEXT,
  ThreadContextPanel,
  useChaos,
  type ChaosSurfaceProps,
} from './surface.tsx'
import css from './Dock.module.css'

/**
 * Docked right-side collaboration surface: opens from the footer Activity
 * entry onto the authoritative Activity inbox; a card click lands in the
 * conversation view (Thread replies take it over full-width); ← returns to
 * the list. Lives in the frame-wide shell.overlay layer and never replaces
 * the app's own columns.
 */
export function ConversationDock(props: ChaosSurfaceProps): React.JSX.Element | null {
  const state = useChaos(props)
  const open = state.dock === 'open'
  const selected = state.targets.find(target => target.id === state.selectedTargetId)
  const panelRef = useRef<HTMLDivElement | null>(null)
  const restoreFocusRef = useRef<Element | null>(null)

  useEffect(() => {
    if (!open) return
    restoreFocusRef.current = document.activeElement
    panelRef.current?.focus()
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') props.closeDock()
    }
    window.addEventListener('keydown', onKeyDown)
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      const previous = restoreFocusRef.current
      if (previous instanceof HTMLElement) previous.focus()
    }
  }, [open]) // eslint-disable-line react-hooks/exhaustive-deps

  if (!open) return null

  if (selected === undefined) {
    return (
      <div
        ref={panelRef}
        className={css.dock}
        role="complementary"
        aria-label="Activity"
        tabIndex={-1}
      >
        <header className={css.dockHead}>
          <span className={css.dockTitle}>Activity</span>
          <button
            type="button"
            className={css.dockClose}
            aria-label="关闭 Activity"
            onClick={() => { props.closeDock() }}
          >
            ✕
          </button>
        </header>
        <ActivityList {...props} state={state} />
      </div>
    )
  }

  const kindGlyph = selected.kind === 'channel' ? '#' : selected.kind === 'direct' ? '@' : '↳'
  const inThread = state.threadPanelId !== undefined
  // Thread targets carry a machine name (thread:<id>); label them by their parent.
  const parentName = selected.kind === 'thread' && selected.parentTargetId !== undefined
    ? state.targets.find(target => target.id === selected.parentTargetId)?.name
    : undefined
  const title = parentName !== undefined ? `${parentName} 的 Thread` : selected.name

  return (
    <div
      ref={panelRef}
      className={css.dock}
      role="complementary"
      aria-label={`协作会话 ${title}`}
      tabIndex={-1}
    >
      <header className={css.dockHead}>
        <button
          type="button"
          className={css.dockBack}
          aria-label="返回 Activity 列表"
          title="返回 Activity 列表"
          onClick={() => { props.backToList() }}
        >
          ←
        </button>
        <span className={css.dockKind}>{kindGlyph}</span>
        <span className={css.dockTitle}>{title}</span>
        <button
          type="button"
          className={css.dockClose}
          aria-label="关闭协作会话"
          onClick={() => { props.closeDock() }}
        >
          ✕
        </button>
      </header>
      {inThread
        ? (
          <div className={css.dockThread}>
            <ThreadContextPanel {...props} state={state} />
          </div>
        )
        : (
          <>
            <MessageList {...props} state={state} />
            <Composer
              {...props}
              state={state}
              draftKey={selected.id}
              onSend={props.send}
              placeholder="回复…"
              showMembers={false}
            />
          </>
        )}
    </div>
  )
}

function ActivityList(props: ChaosSurfaceProps & { state: ChaosClientState }): React.JSX.Element {
  const { state } = props
  const inbox = state.inbox
  const [error, setError] = useState<string | undefined>(undefined)
  const [morePending, setMorePending] = useState(false)

  useEffect(() => {
    if (inbox.status !== 'idle') return
    let cancelled = false
    props.ensure()
      .then(async () => { if (!cancelled) await props.loadInbox() })
      .catch(() => {})
    return () => { cancelled = true }
    // One-shot first load when the list mounts.
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className={css.list}>
      {error !== undefined
        ? <div className={css.listError} role="alert">{error}</div>
        : null}
      {inbox.status === 'loading' && inbox.items.length === 0
        ? <div className={css.empty}>加载中…</div>
        : null}
      {inbox.status === 'error' && inbox.items.length === 0
        ? (
          <div className={css.empty}>
            动态加载失败。
            <button
              type="button"
              className={css.retryButton}
              onClick={() => { void props.loadInbox() }}
            >
              重试
            </button>
          </div>
        )
        : null}
      {inbox.items.map(item => (
        <ActivityCard
          key={item.conversationId}
          {...props}
          item={item}
          onError={setError}
        />
      ))}
      {inbox.status === 'ready' && inbox.items.length === 0
        ? (
          <div className={css.empty}>
            协作动态都处理完了。
            <span className={css.emptyHint}>新的频道消息、Thread 回复和工作项更新会出现在这里。</span>
          </div>
        )
        : null}
      {inbox.nextCursor !== undefined
        ? (
          <button
            type="button"
            className={css.moreButton}
            disabled={morePending}
            onClick={() => {
              setMorePending(true)
              props.loadMoreInbox().finally(() => { setMorePending(false) })
            }}
          >
            {morePending ? '加载中…' : '加载更多'}
          </button>
        )
        : null}
    </div>
  )
}

function ActivityCard(props: ChaosSurfaceProps & {
  state: ChaosClientState
  item: NativeActivityInboxItem
  onError: (message: string) => void
}): React.JSX.Element {
  const { item } = props
  const [donePending, setDonePending] = useState(false)
  const kindGlyph = item.targetKind === 'channel' ? '#' : item.targetKind === 'direct' ? '@' : '↳'
  const replyCount = item.replyCount

  const open = (): void => {
    void props.openDock(item.conversationId).catch((cause: unknown) => {
      props.onError(cause instanceof Error ? cause.message : String(cause))
    })
  }

  const markDone = (): void => {
    if (donePending) return
    setDonePending(true)
    props.markInboxDone(item.conversationId, item.lastActivitySeq)
      .catch((cause: unknown) => {
        props.onError(cause instanceof Error ? cause.message : String(cause))
      })
      .finally(() => { setDonePending(false) })
  }

  return (
    <div
      className={css.card}
      role="button"
      tabIndex={0}
      aria-label={`打开 ${item.targetName}`}
      onClick={open}
      onKeyDown={event => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault()
          open()
        }
      }}
    >
      <div className={css.cardTop}>
        <span className={css.cardTarget}>
          <span className={css.cardKind}>{kindGlyph}</span>
          {item.targetName}
        </span>
        <button
          type="button"
          className={css.doneButton}
          aria-label="标记完成"
          title="标记完成；有新消息时会重新出现"
          disabled={donePending}
          onClick={event => {
            event.stopPropagation()
            markDone()
          }}
        >
          ✓
        </button>
      </div>
      {item.title !== '' ? <div className={css.cardTitle}>{item.title}</div> : null}
      <div className={css.cardMeta}>
        {item.latestReply !== undefined
          ? <span className={css.cardSender}>@{item.latestReply.senderName}</span>
          : null}
        <span>{relTime(item.lastActivityAtMs)}</span>
      </div>
      {item.latestReply !== undefined && item.latestReply.excerpt !== ''
        ? <p className={css.cardExcerpt}>{item.latestReply.excerpt}</p>
        : null}
      {item.task !== undefined || (replyCount !== undefined && replyCount !== '0')
        ? (
          <div className={css.cardChips}>
            {item.task !== undefined
              ? (
                <span className={css.chip} data-status={item.task.status}>
                  工作项 #{item.task.number} · {TASK_STATUS_TEXT[item.task.status] ?? item.task.status}
                </span>
              )
              : null}
            {replyCount !== undefined && replyCount !== '0'
              ? <span className={css.chip}>{replyCount} 条回复</span>
              : null}
          </div>
        )
        : null}
    </div>
  )
}
