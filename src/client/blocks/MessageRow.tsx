/**
 * MessageRow — stream 标准行(含 compact 变体): [28px avatar] name time AGENT-badge /
 * markdown body / task chip / thread 预览泡 / hover 回复钮 (或 compact gutter 时间)。
 * 原 MessageStream 内嵌 .msg/.msgCompact 系 20 余条规则全量平移到此。
 */
import { AvatarChip } from '../atoms/AvatarChip.tsx'
import { TaskChip } from '../atoms/TaskChip.tsx'
import type { NativeActor, NativeMessage, NativeTarget, NativeTask, NativeThreadSummary } from '../../native.ts'
import type { ChaosTranslate } from '../locales.ts'
import { MessageBody } from '../atoms/MessageBody.tsx'
import css from './MessageRow.module.css'
import { IconReply } from '../atoms/DomainIcons.tsx'

/** spec §2.1 preview row: only a committed reply count may create this marker. */
export function ThreadPreview({ t, thread, summary, actorNamesById, onOpen, variant = 'avatar' }: {
  t: ChaosTranslate
  thread: NativeTarget | undefined
  /** Authoritative thread.summaries projection; absent/zero means no marker. */
  summary: NativeThreadSummary | undefined
  actorNamesById: Map<string, string>
  onOpen: (() => void) | undefined
  /** 'avatar'(默认,流)含小头像组; 'plain'=activity 同排号 task chip 写真相。 */
  variant?: 'avatar' | 'plain' | undefined
}): JSX.Element | undefined {
  if (thread === undefined || summary === undefined || summary.replyCount === 0) return undefined
  const content = (
    <>
      <IconReply size={12} />
      {variant === 'avatar' && (
        <span className={css.previewAvatars} aria-hidden="true">
          {summary.recentReplierIds.map(id => (
            <AvatarChip key={id} handle={id} displayName={actorNamesById.get(id) ?? id} size="xxsmall" title={actorNamesById.get(id) ?? id} />
          ))}
        </span>
      )}
      {t('thread.replies', { count: summary.replyCount })}
    </>
  )
  if (onOpen === undefined) {
    return <span className={css.threadPreview} data-plugin="dsh-chaos" data-variant={variant}>{content}</span>
  }
  return (
    <button type="button" className={css.threadPreview} data-plugin="dsh-chaos" data-variant={variant} onClick={onOpen}>
      {content}
    </button>
  )
}

/** Hover reply affordance on the row's right edge (spec §2.1 incl. hover "回复"). */
function ReplyButton({ title, onClick }: { title: string; onClick: () => void }): JSX.Element {
  return (
    <button type="button" className={css.replyButton} data-plugin="dsh-chaos" title={title} aria-label={title} onClick={onClick}>
      <IconReply size={12} />
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
  onOpenTasks: (messageId: string) => void
  onOpenThread: (() => void) | undefined
}): JSX.Element {
  const body = (
    <div className={css.rowBody}>
      <MessageBody t={t} text={message.text} names={mentionNames} />
      {task !== undefined && <TaskChip task={task} assignee={assigneeHandle} onClick={() => { onOpenTasks(message.id) }} />}
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
