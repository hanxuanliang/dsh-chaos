/**
 * Channel message stream (spec §1.2), aligned to plocal-web's MessageScroller /
 * MessageBody anatomy on host tokens:
 * - Rows: full row = 28px avatar + 8px gap + body column (name row + body);
 *   compact rows (same author ≤5min) render a same-width empty placeholder so
 *   both body columns share one left edge, with a hover-revealed absolute
 *   gutter time. mb-1 / px-8 py-1 / hover tint; full rows add mt-1.5.
 * - Sticky centered day-divider pills (Today/Yesterday/month-day-weekday).
 * - Body renderer = react-markdown (remark-gfm + remark-breaks) with a
 *   host-token components map (plocal §4 shapes) and a micro rehype pass for
 *   @mention spans — v10 has no components.text hook (text nodes bypass the
 *   components map), so the split happens on hast text nodes, skipping
 *   code/pre/a subtrees by construction. Images render as links (no external
 *   image fetching in the panel).
 * - plocal TaskChip: status icon + #N (+ @assignee), 4-state icon/color only.
 * - Long bodies clamp at 344→320px with a bottom fade + Show more/Collapse.
 * - Head: "load older" button (forward paging; backend has no before-cursor)
 *   (no "beginning of messages" hint — user asked for a bare top edge); tail
 *   keeps the conn-bar/resync faces.
 * P0 still skips virtual scrolling, thread previews, and hover reply.
 */
import { useEffect, useLayoutEffect, useMemo, useRef, type JSX, type UIEvent } from 'react'
import type { NativeActor, NativeMessage, NativeTarget, NativeTask } from '../native.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './blocks/MessageStream.module.css'
import { MessageRow } from './blocks/MessageRow.tsx'
import { IconBubble } from './atoms/DomainIcons.tsx'

export interface MessageStreamProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channelId: string
  activeLocale(): string
  onOpenTasks(): void
  /** One-shot jump target (task anchor link → land + flash the row). */
  jumpMessageId?: string | undefined
  onJumpHandled?: (() => void) | undefined
  /** Message-row thread entry: preview row / hover reply click (spec §2.1). */
  onOpenThread?: ((messageId: string) => void) | undefined
}

const COMPACT_WINDOW_MS = 5 * 60 * 1000
const SKELETON_ROWS = [0, 1, 2]
/** plocal MessageBody clamp: fold only when the rendered body passes 344px; the
 * 320px collapsed cap lives in CollabPanel.module.css (.msgText[data-clamped]). */

function pad2(value: number): string {
  return String(value).padStart(2, '0')
}

function timeLabel(ms: number): string {
  const date = new Date(ms)
  return `${pad2(date.getHours())}:${pad2(date.getMinutes())}`
}

function sameDay(left: Date, right: Date): boolean {
  return left.getFullYear() === right.getFullYear()
    && left.getMonth() === right.getMonth()
    && left.getDate() === right.getDate()
}

function dividerLabel(ms: number, t: ChaosTranslate, activeLocale: () => string): string {
  const date = new Date(ms)
  const now = new Date()
  if (sameDay(date, now)) return t('stream.today')
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1)
  if (sameDay(date, yesterday)) return t('stream.yesterday')
  return activeLocale() === 'zh'
    ? date.toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric', weekday: 'long' })
    : date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', weekday: 'long' })
}

function fullTimeTitle(ms: number, activeLocale: () => string): string {
  return new Date(ms).toLocaleString(activeLocale() === 'zh' ? 'zh-CN' : 'en-US')
}

interface StreamItem {
  kind: 'divider' | 'message'
  key: string
  label?: string
  message?: NativeMessage
  compact?: boolean
}

function buildItems(messages: NativeMessage[], t: ChaosTranslate, activeLocale: () => string): StreamItem[] {
  const items: StreamItem[] = []
  let previousDay = ''
  let previous: NativeMessage | undefined
  for (const message of messages) {
    const date = new Date(message.createdAtMs)
    const day = `${String(date.getFullYear())}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`
    if (day !== previousDay) {
      items.push({ kind: 'divider', key: `day-${day}`, label: dividerLabel(message.createdAtMs, t, activeLocale) })
      previousDay = day
      previous = undefined
    }
    const compact = previous !== undefined
      && previous.authorId === message.authorId
      && message.createdAtMs - previous.createdAtMs <= COMPACT_WINDOW_MS
    items.push({ kind: 'message', key: message.id, message, compact })
    previous = message
  }
  return items
}

export function MessageStream({ t, store, state, channelId, activeLocale, onOpenTasks, jumpMessageId, onJumpHandled, onOpenThread }: MessageStreamProps): JSX.Element {
  const messages = state.messagesByChannel[channelId]
  const total = state.totalByChannel[channelId]
  const actorsById = useMemo(() => {
    const mapped = new Map<string, NativeActor>()
    for (const actor of state.actors) mapped.set(actor.id, actor)
    return mapped
  }, [state.actors])
  const actorNamesById = useMemo(() => {
    const mapped = new Map<string, string>()
    for (const actor of state.actors) mapped.set(actor.id, actor.displayName)
    return mapped
  }, [state.actors])
  const mentionNames = useMemo(() => {
    const names = new Set<string>()
    for (const actor of state.actors) {
      names.add(actor.handle.toLowerCase())
      names.add(actor.displayName.toLowerCase())
    }
    return names
  }, [state.actors])
  const items = useMemo(
    () => buildItems(messages ?? [], t, activeLocale),
    [messages, t, activeLocale],
  )
  /** Thread target per root message, when one exists. */
  const threadsByRoot = useMemo(() => {
    const mapped = new Map<string, NativeTarget>()
    for (const thread of state.threads) {
      if (thread.rootMessageId !== undefined) mapped.set(thread.rootMessageId, thread)
    }
    return mapped
  }, [state.threads])

  // Auto-scroll: stick to the bottom while the user is near it; keep the
  // viewport anchored by height delta across a "load older" prepend.
  const scrollerRef = useRef<HTMLDivElement | null>(null)
  const pinnedRef = useRef(true)
  const channelRef = useRef(channelId)
  const olderAnchor = useRef<{ loading: boolean; height: number }>({ loading: false, height: 0 })

  useLayoutEffect(() => {
    const el = scrollerRef.current
    if (el === null) return
    if (channelRef.current !== channelId) {
      channelRef.current = channelId
      pinnedRef.current = true
    }
    const anchor = olderAnchor.current
    if (state.olderLoading) {
      anchor.loading = true
      anchor.height = el.scrollHeight
    } else if (anchor.loading) {
      anchor.loading = false
      el.scrollTop += el.scrollHeight - anchor.height
    }
    if (pinnedRef.current) el.scrollTop = el.scrollHeight
  }, [channelId, messages, state.olderLoading])

  // Task-anchor jump: scroll the target row into view and flash it. If the
  // row is not in the merged window yet, page older until it appears (or the
  // channel is fully backfilled); handled jumps are reported exactly once.
  const jumpHandledRef = useRef<string | undefined>(undefined)
  useEffect(() => {
    if (jumpMessageId === undefined || jumpHandledRef.current === jumpMessageId) return
    const el = scrollerRef.current
    if (el === null) return
    const row = el.querySelector(`[data-message-id="${jumpMessageId}"]`)
    if (row !== null) {
      jumpHandledRef.current = jumpMessageId
      pinnedRef.current = false
      row.scrollIntoView({ block: 'center' })
      row.setAttribute('data-jump-flash', '')
      window.setTimeout(() => { row.removeAttribute('data-jump-flash') }, 1600)
      onJumpHandled?.()
      return
    }
    const channelMessages = state.messagesByChannel[channelId]
    const totalCount = state.totalByChannel[channelId]
    const hasMoreToLoad = channelMessages !== undefined && totalCount !== undefined && channelMessages.length < totalCount
    if (!state.olderLoading) {
      if (hasMoreToLoad) {
        void store.loadOlder()
      } else {
        jumpHandledRef.current = jumpMessageId
        onJumpHandled?.()
      }
    }
  }, [jumpMessageId, state.messagesByChannel, state.totalByChannel, state.olderLoading, channelId, store, onJumpHandled])

  const onScroll = (event: UIEvent<HTMLDivElement>): void => {
    const el = event.currentTarget
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60
  }

  if (state.connection === 'resyncing') {
    return (
      <div className={css.stream} role="status">
        <div className={css.chatCol}>
          <p className={css.resyncNote}>{t('channel.resyncing')}</p>
          {SKELETON_ROWS.map(row => <div key={row} className={css.skeletonRow} />)}
        </div>
      </div>
    )
  }

  const hasMore = messages !== undefined && total !== undefined && messages.length < total

  return (
    <div className={css.streamWrap}>
      {state.connection === 'down' && (
        <div className={css.connBar} role="status">{t('channel.disconnected')}</div>
      )}
      <div className={css.stream} ref={scrollerRef} onScroll={onScroll}>
        <div className={css.chatCol}>
          {state.historyLoading && messages === undefined && (
            <div className={css.skeletonStack} role="status" aria-label={t('channel.loading')}>
              {SKELETON_ROWS.map(row => <div key={row} className={css.skeletonRow} />)}
            </div>
          )}
          {messages !== undefined && messages.length === 0 && !state.historyLoading && state.historyError === undefined && (
            <div className={css.streamEmpty}>
              <IconBubble />
              <p>{t('channel.empty')}</p>
            </div>
          )}
          {hasMore && (
            <div className={css.olderRow}>
              {state.olderLoading
                ? SKELETON_ROWS.map(row => <div key={row} className={css.skeletonRow} />)
                : (
                  <button type="button" className={css.olderButton} onClick={() => { void store.loadOlder() }}>
                    {t('channel.loadOlder')}
                  </button>
                )}
            </div>
          )}
            <div className={css.listPush} aria-hidden="true" />
          {items.map((item) => {
            if (item.kind === 'divider') {
              return <div key={item.key} className={css.dayDivider}><span>{item.label}</span></div>
            }
            const message = item.message as NativeMessage
            const author = actorsById.get(message.authorId)
            const task: NativeTask | undefined = state.tasksByMessage[message.id]
            const showTask = task !== undefined && task.targetId === channelId
            const assignee = showTask && task.assigneeId !== undefined
              ? actorsById.get(task.assigneeId)?.handle
              : undefined
            const binding = author === undefined ? undefined : state.bindingsByAgent[author.id]
            return (
              <MessageRow
                key={item.key}
                t={t}
                message={message}
                compact={item.compact === true}
                author={author}
                bindingModel={binding?.model}
                task={showTask ? task : undefined}
                assigneeHandle={assignee}
                thread={threadsByRoot.get(message.id)}
                summary={state.threadSummariesByRoot[message.id]}
                actorNamesById={actorNamesById}
                mentionNames={mentionNames}
                timeText={timeLabel(message.createdAtMs)}
                fullTimeTitle={fullTimeTitle(message.createdAtMs, activeLocale)}
                onOpenTasks={onOpenTasks}
                onOpenThread={onOpenThread === undefined ? undefined : () => { onOpenThread(message.id) }}
              />
            )
          })}
        </div>
      </div>
    </div>
  )
}
