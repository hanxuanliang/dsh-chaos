/**
 * ActivityCard — inbox 行卡: 目标行(#父 + 相对时间)→主标题 bold 2-clamp
 * → 最新回复 → 底行 StatusChip + N replies; 右悬停靠 ✓ done。rowMain 负责
 * 打开 dock (channel/thread 双类由 caller 分派)。
 */
import { IconCheckOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import { TaskChip } from '../atoms/TaskChip.tsx'
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
  onOpen: (trigger?: HTMLButtonElement) => void
  onDone: () => void
}) {
  return (
    <div className={css.row} role="listitem" data-selected={selected ? 'true' : undefined} data-done={item.done === true ? 'true' : undefined}>
      <button type="button" className={css.rowMain} data-activity-id={item.conversationId} onClick={event => { onOpen(event.currentTarget) }}>
        <span className={css.body}>
          <span className={css.titleLine}>
            {item.done !== true && <span className={css.unreadDot} aria-hidden="true" />}
            <span className={css.titleText}>{item.title}</span>
          </span>
          <span className={css.summaryLine}>
            {item.latestReply !== undefined
              ? <ReplyExcerpt senderName={item.latestReply.senderName} excerpt={item.latestReply.excerpt} />
              : <span />}
            <span className={css.time}>{timeLabel}</span>
          </span>
          {(item.task !== undefined || item.targetKind === 'thread') && (
            <span className={css.bottomLine}>
              {item.task !== undefined && (
                <TaskChip
                  task={item.task}
                  assignee={item.task.assigneeName}
                />
              )}
              {item.targetKind === 'thread' && (
                <ThreadPreview
                  t={t}
                  thread={thread}
                  summary={summary}
                  actorNamesById={actorNamesById}
                  onOpen={undefined}
                  variant="plain"
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
