/**
 * Activity inbox view (crates inbox_list, plocal InboxMain 形态对齐):
 * 会话粒度行卡 —— channel 行标题 = 频道名,thread 行标题 = root 摘要
 * (crates 已合入 title/titleKind);副行最近的回复者:摘录;尾相对时间 +
 * thread 的 N replies;root 锚了 task 则贴状态 chip;行右上 ✓ = crates
 * done_through_seq(不是 read: 新活动自动复活)。
 * direct 行暂不做(DM UI 未建,不做假跳转自己)。
 */
import { useMemo, useState, type JSX } from 'react'
import type { NativeActivityInboxItem } from '../native.ts'
import css from './CollabPanel.module.css'
import type { ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'

interface ActivityViewProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  onOpenChannel: (channelId: string) => void
  onOpenThreadRoot: (rootMessageId: string, parentChannelId: string) => void
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
  return new Date(atMs).toLocaleDateString()
}

function KindGlyph({ kind }: { kind: NativeActivityInboxItem['targetKind'] }): JSX.Element {
  if (kind === 'channel') {
    return (
      <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M3.5 5.5h9M3.5 10.5h9M6.5 3 5 13M11 3 9.5 13" />
      </svg>
    )
  }
  if (kind === 'thread') {
    return (
      <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M6 11 2.5 7.5 6 4M2.5 7.5h6a3.5 3.5 0 0 1 3.5 3.5v2" />
      </svg>
    )
  }
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
      <circle cx="8" cy="8" r="4.5" />
    </svg>
  )
}

export function ActivityView({ t, store, state, onOpenChannel, onOpenThreadRoot }: ActivityViewProps): JSX.Element {
  const [busy, setBusy] = useState<string | undefined>(undefined)
  const [error, setError] = useState<string | undefined>(undefined)

  // direct 行 P0 不做(DM 主界面没建,点击没有诚实目标 — 直接隐藏)。
  const items = useMemo(
    () => state.activityItems.filter(item => item.targetKind !== 'direct'),
    [state.activityItems],
  )

  const open = (item: NativeActivityInboxItem): void => {
    if (item.targetKind === 'channel') {
      onOpenChannel(item.conversationId)
      return
    }
    if (item.targetKind === 'thread' && item.rootMessageId !== undefined && item.parentTargetId !== undefined) {
      onOpenThreadRoot(item.rootMessageId, item.parentTargetId)
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

  return (
    <div className={css.activityView}>
      <header className={css.activityHeader}>
        <h2 className={css.activityTitle}>{t('activity.title')}</h2>
        <span className={css.activityCount}>{state.activityCount}</span>
      </header>
      {error !== undefined && <div className={css.taskBoardError} role="alert">{error}</div>}
      {items.length === 0 ? (
        <div className={css.activityEmpty}>{t('activity.empty')}</div>
      ) : (
        <div className={css.activityList} role="list">
          {items.map((item) => (
            <div key={item.conversationId} className={css.activityRow} role="listitem">
              <button type="button" className={css.activityRowMain} onClick={() => { open(item) }}>
                <span className={css.activityKind} data-kind={item.targetKind}>
                  <KindGlyph kind={item.targetKind} />
                </span>
                <span className={css.activityBody}>
                  <span className={css.activityTitleLine}>
                    <span className={css.activityTitleText}>{item.title}</span>
                    {item.task !== undefined && (
                      <span className={css.statusChip} data-status={item.task.status}>
                        <span className={css.statusDot} data-status={item.task.status} aria-hidden="true" />
                        {item.task.assigneeName !== undefined ? `@${item.task.assigneeName}` : `#${item.task.number}`}
                      </span>
                    )}
                    <span className={css.activityTime}>{relativeTime(item.lastActivityAtMs)}</span>
                  </span>
                  <span className={css.activityMeta}>
                    <span className={css.activityTarget}>{
                      item.targetKind === 'thread' ? `#${item.targetName}` : item.targetName
                    }</span>
                    {item.latestReply !== undefined && (
                      <span className={css.activityExcerpt}>
                        {item.latestReply.senderName}: {item.latestReply.excerpt}
                      </span>
                    )}
                  </span>
                </span>
                {item.replyCount !== undefined && item.targetKind === 'thread' && (
                  <span className={css.activityReplies}>{t('thread.replies', { count: Number(item.replyCount) })}</span>
                )}
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
      )}
    </div>
  )
}
