/**
 * ActivityCard — inbox 行卡: 目标行(#父 + 相对时间)→主标题 bold 2-clamp
 * → 最新回复 → 底行 StatusChip + N replies; 右悬停靠 ✓ done。rowMain 负责
 * 打开 dock (channel/thread 双类由 caller 分派)。
 */
import { IconCheckOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import { StatusChip } from '../atoms/StatusChip.tsx'
import { ReplyExcerpt } from '../atoms/ReplyExcerpt.tsx'
import { ThreadPreview } from './MessageRow.tsx'
import type { NativeActivityInboxItem, NativeTarget, NativeThreadSummary } from '../../native.ts'
import type { ChaosTranslate } from '../locales.ts'
import css from './ActivityCard.module.css'

export function ActivityCard({ item, t, timeLabel, selected, busy, thread, summary, actorNamesById, onOpen, onDone }: {
  item: NativeActivityInboxItem
  t: ChaosTranslate
  /** 相对时间文案由 caller 组好(relativeTime)。 */
  timeLabel: string
  selected: boolean
  busy: boolean
  thread: NativeTarget | undefined
  summary: NativeThreadSummary | undefined
  actorNamesById: Map<string, string>
  onOpen: () => void
  onDone: () => void
}) {
  return (
    <div className={css.row} role="listitem" data-selected={selected ? 'true' : undefined}>
      <button type="button" className={css.rowMain} onClick={onOpen}>
        <span className={css.body}>
          <span className={css.line1}>
            <span className={css.time}>{timeLabel}</span>
          </span>
          <span className={css.titleText}>{item.title}</span>
          {item.latestReply !== undefined && (
            <ReplyExcerpt senderName={item.latestReply.senderName} excerpt={item.latestReply.excerpt} />
          )}
          {(item.task !== undefined || item.targetKind === 'thread') && (
            <span className={css.bottomLine}>
              {item.task !== undefined && (
                <StatusChip
                  status={item.task.status}
                  label={item.task.assigneeName !== undefined ? `@${item.task.assigneeName}` : `#${item.task.number}`}
                />
              )}
              {item.targetKind === 'thread' && (
                <ThreadPreview
                  t={t}
                  thread={thread}
                  summary={summary}
                  actorNamesById={actorNamesById}
                  onOpen={onOpen}
                />
              )}
            </span>
          )}
        </span>
      </button>
      <button
        type="button"
        className={css.done}
        title={t('activity.done')}
        aria-label={t('activity.done')}
        disabled={busy}
        onClick={onDone}
      >
        <IconCheckOutline16 />
      </button>
    </div>
  )
}
