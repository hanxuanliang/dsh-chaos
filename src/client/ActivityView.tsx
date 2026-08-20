/**
 * Activity 视图(rc-4 统一性整改):
 * - 行卡: 目标小行 → 主标题(粗) → 最新回复 → 底行(task chip+N replies);
 *   不再有前置 kind 图标(点击自然开右栏)。
 * - 点击 = 列表收窄 + 右栏 dock,channel/thread 同一套 dock 机制, 不再整页跳:
 *   channel → DockedChannelPane(header-lite + MessageStream + ChannelComposer);
 *   thread → ThreadPanel 本体(与频道内同一件)。
 * - 组件复用: Button pill 与频道 tab 同一 css.tab 族; 状态色块 = StatusChip
 *   (与看板 toggle 同源); composer = ChannelComposer; 流 = MessageStream。
 * - 筛选 pill: All 生效; Unread/Mentions disabled(plocal 此刻同样 disabled —
 *   read vertical 未落地), Mark-all-read 同因 disabled。不做假交互。
 * - direct 行暂不做(DM 主界面没建,点击没有诚实目标 — 隐藏)。
 */
import { useMemo, useState, type JSX } from 'react'
import type { NativeActivityInboxItem, NativeTarget } from '../native.ts'
import css from './CollabPanel.module.css'
import type { ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import { ThreadPanel } from './ThreadPanel.tsx'
import { MessageStream } from './MessageStream.tsx'
import { ChannelComposer } from './ChannelComposer.tsx'
import { StatusChip } from './StatusChip.tsx'

interface ActivityViewProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  onOpenThreadRoot: (rootMessageId: string, parentChannelId: string) => void
  activeLocale(): string
}

/** dock 状态: 会话粒度(item 行) —— 但 channel dock 内部还能下钻一个 thread。 */
type Dock =
  | { kind: 'channel'; channelId: string }
  | { kind: 'thread'; rootMessageId: string; parentChannelId: string }

function relativeTime(atMs: number): string {
  const deltaSeconds = Math.max(0, Math.floor((Date.now() - atMs) / 1000))
  if (deltaSeconds < 60) return '1m'
  const minutes = Math.floor(deltaSeconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  return new Date(atMs).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

function DockedChannelPane({ t, store, state, channel, activeLocale, onOpenThread, onClose }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channel: NativeTarget
  activeLocale(): string
  onOpenThread(rootMessageId: string): void
  onClose(): void
}): JSX.Element {
  return (
    <div className={css.dockedChannel}>
      <header className={css.channelHead}>
        <h3 className={css.channelTitle}>
          <span className={css.channelHash} aria-hidden="true">#</span>
          {channel.name}
        </h3>
        {/* ✕ 与 ThreadPanel 头部同一件 .threadClose, 不再自造 */}
        <button type="button" className={css.threadClose} aria-label={t('activity.closeDock')} title={t('activity.closeDock')} onClick={onClose}>
          <svg viewBox="0 0 16 16" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
            <path d="m4 4 8 8M12 4l-8 8" />
          </svg>
        </button>
      </header>
      <MessageStream
        t={t} store={store} state={state} channelId={channel.id} activeLocale={activeLocale}
        onOpenTasks={() => { /* dock 里没有 tasks tab — 诚实不做假入口 */ }}
        onOpenThread={onOpenThread}
      />
      <div className={css.composerSeat}>
        <ChannelComposer t={t} store={store} state={state} channel={channel} disabled={state.connection !== 'live'} />
      </div>
    </div>
  )
}

export function ActivityView({ t, store, state, onOpenThreadRoot, activeLocale }: ActivityViewProps): JSX.Element {
  const [busy, setBusy] = useState<string | undefined>(undefined)
  const [error, setError] = useState<string | undefined>(undefined)
  const [dock, setDock] = useState<Dock | undefined>(undefined)

  const items = useMemo(
    () => state.activityItems.filter((item) => item.targetKind !== 'direct'),
    [state.activityItems],
  )

  const dockThread = dock?.kind === 'thread'
    ? state.threads.find((thread) => thread.rootMessageId === dock.rootMessageId)
    : undefined

  const dockKey = dock === undefined
    ? undefined
    : dock.kind === 'channel' ? `c:${dock.channelId}` : `t:${dock.rootMessageId}`

  const open = (item: NativeActivityInboxItem): void => {
    if (item.targetKind === 'channel') {
      void store.hydrateTarget(item.conversationId)
      setDock((cur) =>
        cur !== undefined && cur.kind === 'channel' && cur.channelId === item.conversationId
          ? undefined
          : { kind: 'channel', channelId: item.conversationId },
      )
      return
    }
    if (item.targetKind === 'thread' && item.rootMessageId !== undefined && item.parentTargetId !== undefined) {
      const rootMessageId = item.rootMessageId
      const parentChannelId = item.parentTargetId
      void store.openThread(rootMessageId)
      void store.hydrateTarget(parentChannelId) // root 卡取自父频道历史
      setDock((cur) =>
        cur !== undefined && cur.kind === 'thread' && cur.rootMessageId === rootMessageId
          ? undefined
          : { kind: 'thread', rootMessageId, parentChannelId },
      )
    }
  }

  const markDone = (item: NativeActivityInboxItem): void => {
    setBusy(item.conversationId)
    setError(undefined)
    store.markActivityDone(item.conversationId, item.lastActivitySeq)
      .catch((e: unknown) => {
        setError(t('activity.doneFailed', { error: e instanceof Error ? e.message : String(e) }))
      })
      .finally(() => { setBusy(undefined) })
  }

  const isDockSelected = (item: NativeActivityInboxItem): boolean => {
    if (dock === undefined) return false
    if (item.targetKind === 'channel') return dock.kind === 'channel' && dock.channelId === item.conversationId
    return dock.kind === 'thread' && item.rootMessageId !== undefined && dock.rootMessageId === item.rootMessageId
  }

  const rows = (
    <div className={css.activityList} role="list">
      {items.map((item) => (
        <div key={item.conversationId} className={css.activityRow} role="listitem" data-selected={isDockSelected(item) ? 'true' : undefined}>
          <button type="button" className={css.activityRowMain} onClick={() => { open(item) }}>
            <span className={css.activityBody}>
              <span className={css.activityLine1}>
                <span className={css.activityTarget}>#{item.targetName}</span>
                <span className={css.activityTime}>{relativeTime(item.lastActivityAtMs)}</span>
              </span>
              <span className={css.activityTitleText}>{item.title}</span>
              {item.latestReply !== undefined && (
                <span className={css.activityExcerpt}>
                  {item.latestReply.senderName}: {item.latestReply.excerpt}
                </span>
              )}
              {(item.task !== undefined || item.targetKind === 'thread') && (
                <span className={css.activityBottomLine}>
                  {item.task !== undefined && (
                    <StatusChip
                      status={item.task.status}
                      label={item.task.assigneeName !== undefined ? `@${item.task.assigneeName}` : `#${item.task.number}`}
                    />
                  )}
                  {item.targetKind === 'thread' && item.replyCount !== undefined && (
                    <span className={css.activityReplies}>{t('thread.replies', { count: Number(item.replyCount) })}</span>
                  )}
                </span>
              )}
            </span>
          </button>
          <button
            type="button"
            className={css.activityDone}
            title={t('activity.done')}
            aria-label={t('activity.done')}
            disabled={busy === item.conversationId}
            onClick={() => { markDone(item) }}
          >
            <svg viewBox="0 0 16 16" width="13" height="13" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="m3.5 8.5 3 3 6-7" />
            </svg>
          </button>
        </div>
      ))}
    </div>
  )

  const filterTabs = (
    <span className={css.tabs} role="tablist" aria-label={t('activity.filtersAria')}>
      <button type="button" role="tab" aria-selected="true" data-active="true" className={css.tab}>{t('activity.filterAll')}</button>
      <button type="button" role="tab" aria-selected="false" disabled className={css.tab} title={t('activity.filterPending')}>{t('activity.filterUnread')}</button>
      <button type="button" role="tab" aria-selected="false" disabled className={css.tab} title={t('activity.filterPending')}>{t('activity.filterMentions')}</button>
    </span>
  )

  return (
    <div className={css.activityView}>
      <header className={css.activityHeader}>
        <h2 className={css.activityTitle}>{t('activity.title')}</h2>
        <span className={css.activityCount}>{t('activity.activeSummary', { count: state.activityCount })}</span>
        <span className={css.activityHeaderRight}>
          {filterTabs}
          <button type="button" className={css.tab} disabled title={t('activity.filterPending')}>{t('activity.markAllRead')}</button>
        </span>
      </header>
      {error !== undefined && <div className={css.taskBoardError} role="alert">{error}</div>}
      {items.length === 0 ? (
        <div className={css.activityEmpty}>{t('activity.empty')}</div>
      ) : dock !== undefined ? (
        <div className={css.activitySplit}>
          <div className={css.activityListPane}>{rows}</div>
          <div className={css.activityDetailPane} key={dockKey}>
            {dock.kind === 'channel' ? (
              <DockedChannelPane
                t={t}
                store={store}
                state={state}
                channel={state.channels.find((c) => c.id === dock.channelId) as NativeTarget}
                activeLocale={activeLocale}
                onOpenThread={(rootMessageId) => {
                  void store.openThread(rootMessageId)
                  setDock({ kind: 'thread', rootMessageId, parentChannelId: dock.channelId })
                }}
                onClose={() => { setDock(undefined) }}
              />
            ) : dockThread !== undefined ? (
              <ThreadPanel
                t={t}
                store={store}
                state={state}
                thread={dockThread}
                parentChannelId={dock.parentChannelId}
                activeLocale={activeLocale}
                onRootJump={() => {
                  if (dock.kind === 'thread') onOpenThreadRoot(dock.rootMessageId, dock.parentChannelId)
                }}
                onClose={() => { setDock(undefined) }}
              />
            ) : (
              <div className={css.activityEmpty}>{t('activity.empty')}</div>
            )}
          </div>
        </div>
      ) : (
        rows
      )}
    </div>
  )
}
