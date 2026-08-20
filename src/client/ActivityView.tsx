/**
 * Activity inbox view — plocal InboxMain / raft Activity panel 形态:
 * - 列表行卡三段式: ① 小目标行 (@the-name / #channel + 相对时间, 灰);
 *   ② 概要主行 (thread=root 摘录, channel=最新条); ③ 次行 latest reply;
 *   ④ 底行: root 锚了 task → 状态 chip; `N replies`。
 * - 点开 thread 行 = 左侧列收窄 + 右栏挂 ThreadPanel 本体 (与频道内一致),
 *   不跳出来。channel 行跳回频道(plocal 打开行走聊天区对应)。
 * - direct 行暂不做(DM 主界面没建,点击没有诚实目标 — 隐藏)。
 * - 工具 pill All/Unread/Mentions: 仅 All 真实生效(plocal 此刻也是 disabled),
 *   且参考图按此形态开放 —— 等 read vertical 上了再开。
 * - Done ✓ = crates inbox_done(done_through_seq per actor, 新活动自动复活)。
 */
import { useMemo, useState, type JSX } from 'react'
import type { NativeActivityInboxItem } from '../native.ts'
import css from './CollabPanel.module.css'
import type { ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import { ThreadPanel } from './ThreadPanel.tsx'
import { avatarSeed } from './avatar.ts'

interface ActivityViewProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  onOpenChannel: (channelId: string) => void
  onOpenThreadRoot: (rootMessageId: string, parentChannelId: string) => void
  activeLocale(): string
}

function relativeTime(atMs: number): string {
  const deltaSeconds = Math.max(0, Math.floor((Date.now() - atMs) / 1000))
  if (deltaSeconds < 60) return '1m'
  const minutes = Math.floor(deltaSeconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  if (days < 2) return t_const('Yesterday')
  return new Date(atMs).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

/** 防止 daily 常量区地方性问题: 相对时间用的今天内默认格式(固定英文,
 * 中文界面也过 plocal 形态)。 */
function t_const(s: string): string { return s }


export function ActivityView({ t, store, state, onOpenChannel, onOpenThreadRoot, activeLocale }: ActivityViewProps): JSX.Element {
  const [busy, setBusy] = useState<string | undefined>(undefined)
  const [error, setError] = useState<string | undefined>(undefined)
  /** 打开的 thread 行 conversationId; undefined = 列表全宽。 */
  const [openConversationId, setOpenConversationId] = useState<string | undefined>(undefined)

  const items = useMemo(
    () => state.activityItems.filter((item) => item.targetKind !== 'direct'),
    [state.activityItems],
  )

  const openItem = items.find((item) => item.conversationId === openConversationId)
  const openThread = openItem !== undefined && openItem.targetKind === 'thread' && openItem.parentTargetId !== undefined
    ? state.threads.find((thread) => thread.rootMessageId === openItem.rootMessageId)
    : undefined

  const open = (item: NativeActivityInboxItem): void => {
    if (item.targetKind === 'channel') {
      onOpenChannel(item.conversationId)
      return
    }
    if (item.targetKind === 'thread' && item.rootMessageId !== undefined && item.parentTargetId !== undefined) {
      // 留在 Activity 视图: 列表收窄 + 右栏 ThreadPanel — plocal/raft 的形态。
      setOpenConversationId((cur) => (cur === item.conversationId ? undefined : item.conversationId))
      void store.openThread(item.rootMessageId)
      // ThreadPanel 的 root 卡取自父观点历史: 若还没加载, 顺带拉 (不影响 active)。
      void store.hydrateTarget(item.parentTargetId)
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

  const rows = (
    <div className={css.activityList} role="list">
      {items.map((item) => (
        <div key={item.conversationId} className={css.activityRow} role="listitem" data-selected={item.conversationId === openConversationId ? 'true' : undefined}>
          <button type="button" className={css.activityRowMain} onClick={() => { open(item) }}>
            <span className={css.activityKindAvatar} aria-hidden="true">
              {item.targetKind === 'channel'
                ? <span className={css.avatarXs} style={{ background: avatarSeed(item.targetName, item.targetName).background }}>#</span>
                : item.targetKind === 'thread'
                  ? <span className={css.avatarXs} style={{ background: avatarSeed(item.targetName, item.targetName).background }}>↩</span>
                  : <span className={css.avatarXs} style={{ background: avatarSeed(item.targetName, item.targetName).background }}>@</span>}
            </span>
            <span className={css.activityBody}>
              <span className={css.activityLine1}>
                <span className={css.activityTarget}>{
                  item.targetKind === 'thread' ? `#${item.targetName}` : `#${item.targetName}`
                }</span>
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
                    <span className={css.statusChip} data-status={item.task.status}>
                      <span className={css.statusDot} data-status={item.task.status} aria-hidden="true" />
                      {item.task.assigneeName !== undefined ? `@${item.task.assigneeName}` : `#${item.task.number}`}
                    </span>
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

  return (
    <div className={css.activityView}>
      <header className={css.activityHeader}>
        <span className={css.activityMark} aria-hidden="true">A</span>
        <h2 className={css.activityTitle}>{t('activity.title')}</h2>
        <span className={css.activityCount}>{t('activity.activeSummary', { count: state.activityCount })}</span>
        <span className={css.activityFilters} role="tablist" aria-label={t('activity.filtersAria')}>
          <button type="button" role="tab" aria-selected="true" data-active="true" className={css.activityFilter}>{t('activity.filterAll')}</button>
          <button type="button" role="tab" aria-selected="false" disabled className={css.activityFilter} title={t('activity.filterPending')}>{t('activity.filterUnread')}</button>
          <button type="button" role="tab" aria-selected="false" disabled className={css.activityFilter} title={t('activity.filterPending')}>{t('activity.filterMentions')}</button>
        </span>
        <button type="button" className={css.activityMarkAll} disabled title={t('activity.filterPending')}>{t('activity.markAllRead')}</button>
      </header>
      {error !== undefined && <div className={css.taskBoardError} role="alert">{error}</div>}
      {items.length === 0 ? (
        <div className={css.activityEmpty}>{t('activity.empty')}</div>
      ) : openThread !== undefined && openItem !== undefined && openItem.parentTargetId !== undefined ? (
        <div className={css.activitySplit}>
          <div className={css.activityListPane}>{rows}</div>
          <div className={css.activityDetailPane}>
            <ThreadPanel
              t={t}
              store={store}
              state={state}
              thread={openThread}
              parentChannelId={openItem.parentTargetId}
              activeLocale={activeLocale}
              onRootJump={() => { onOpenThreadRoot(openItem.rootMessageId as string, openItem.parentTargetId as string) }}
              onClose={() => { setOpenConversationId(undefined) }}
            />
          </div>
        </div>
      ) : (
        rows
      )}
    </div>
  )
}
