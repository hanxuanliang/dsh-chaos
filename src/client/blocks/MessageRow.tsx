/**
 * MessageRow — stream 标准行(含 compact 变体): [28px avatar] name time AGENT-badge /
 * markdown body / task chip / thread 预览泡 / hover 回复钮 (或 compact gutter 时间)。
 * 原 MessageStream 内嵌 .msg/.msgCompact 系 20 余条规则全量平移到此。
 */
import { AvatarChip } from '../atoms/AvatarChip.tsx'
import { TaskChip } from '../atoms/TaskChip.tsx'
import type { NativeActor, NativeMessage, NativeTarget, NativeTask, NativeThreadSummary } from '../../native.ts'
import type { ChaosTranslate } from '../locales.ts'
import { MessageBody } from '../MessageStream.tsx'
import css from './MessageRow.module.css'

/** spec §2.1 preview row: ↩ N 条回复 — count only when the total is known; never invented. */
export function ThreadPreview({ t, thread, summary, actorNamesById, onOpen }: {
  t: ChaosTranslate
  thread: NativeTarget | undefined
  /** tae thread.summaries 批量投影；未知时回退纯「打开线程」。 */
  summary: NativeThreadSummary | undefined
  actorNamesById: Map<string, string>
  onOpen: (() => void) | undefined
}): JSX.Element | undefined {
  if (thread === undefined) return undefined
  return (
    <button type="button" className={css.threadPreview} data-plugin="dsh-chaos" onClick={onOpen} disabled={onOpen === undefined}>
      <svg viewBox="0 0 16 16" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M6 11 2.5 7.5 6 4M2.5 7.5h6a3.5 3.5 0 0 1 3.5 3.5v2" />
      </svg>
      {summary === undefined ? (
        t('thread.openThread')
      ) : (
        <>
          {/* tae/plocal 终局形态：头像组（最近 3 位回复者）+ 计数；不含回复正文。 */}
          <span className={css.previewAvatars} aria-hidden="true">
            {summary.recentReplierIds.map(id => (
              <AvatarChip key={id} handle={id} displayName={actorNamesById.get(id) ?? id} size="xxsmall" title={actorNamesById.get(id) ?? id} />
            ))}
          </span>
          {t('thread.replies', { count: summary.replyCount })}
        </>
      )}
    </button>
  )
}

/** Hover reply affordance on the row's right edge (spec §2.1 incl. hover "回复"). */
function ReplyButton({ title, onClick }: { title: string; onClick: () => void }): JSX.Element {
  return (
    <button type="button" className={css.replyButton} data-plugin="dsh-chaos" title={title} aria-label={title} onClick={onClick}>
      <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M6 11 2.5 7.5 6 4M2.5 7.5h6a3.5 3.5 0 0 1 3.5 3.5v2" />
      </svg>
    </button>
  )
}

export function MessageRow({ t, message, compact = false, author, bindingModel, task, assigneeHandle, thread, summary, actorNamesById, mentionNames, timeText, fullTimeTitle, onOpenTasks, onOpenThread }: {
  t: ChaosTranslate
  message: NativeMessage
  /** 同一天同人 5 分钟内 → 无头行(headless): 左 gutter 时间(hover)。 */
  compact?: boolean | undefined
  author: NativeActor | undefined
  bindingModel: string | undefined
  task: NativeTask | undefined
  assigneeHandle: string | undefined
  thread: NativeTarget | undefined
  summary: NativeThreadSummary | undefined
  actorNamesById: Map<string, string>
  mentionNames: ReadonlySet<string>
  timeText: string
  fullTimeTitle: string
  onOpenTasks: () => void
  onOpenThread: (() => void) | undefined
}): JSX.Element {
  const body = (
    <div className={css.rowBody}>
      <MessageBody t={t} text={message.text} names={mentionNames} />
      {task !== undefined && <TaskChip task={task} assignee={assigneeHandle} onClick={onOpenTasks} />}
      <ThreadPreview t={t} thread={thread} summary={summary} actorNamesById={actorNamesById} onOpen={onOpenThread} />
    </div>
  )
  if (compact) {
    return (
      <div className={css.rowCompact} data-message-id={message.id}>
        <div className={css.avatarPlaceholder} aria-hidden="true" />
        {body}
        {onOpenThread !== undefined && <ReplyButton title={t('thread.reply')} onClick={onOpenThread} />}
        <span className={css.gutterTime} title={fullTimeTitle}>{timeText}</span>
      </div>
    )
  }
  const handle = author?.handle ?? message.authorId
  const displayName = author?.displayName ?? message.authorId
  return (
    <div className={css.row} data-message-id={message.id}>
      <AvatarChip handle={handle} displayName={displayName} size="lg" />
      <div className={css.main}>
        <div className={css.head}>
          <span className={css.name}>{displayName}</span>
          <span className={css.time}>{timeText}</span>
          {author?.kind === 'agent' && (
            <span className={css.badge}>
              AGENT{bindingModel !== undefined ? ` · ${bindingModel}` : ''}
            </span>
          )}
        </div>
        {body}
      </div>
      {onOpenThread !== undefined && <ReplyButton title={t('thread.reply')} onClick={onOpenThread} />}
    </div>
  )
}
