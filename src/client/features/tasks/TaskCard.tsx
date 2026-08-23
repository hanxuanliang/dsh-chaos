/** Compact Board card: identity/time, title/summary, then assignee/source. */
import { forwardRef } from 'react'
import { IconLinkOutline14 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeTask } from '../../../native.ts'
import { AvatarChip } from '../../shared/ui/AvatarChip.tsx'
import css from './TaskCard.module.css'

export const TaskCard = forwardRef<HTMLButtonElement, {
  task: NativeTask
  title: string
  excerpt: string
  assignee: NativeActor | undefined
  unassignedLabel: string
  timeLabel: string
  sourceLabel: string
  dragging: boolean
  draggable?: boolean | undefined
  highlighted: boolean
  onDragStart: () => void
  onDragEnd: () => void
  onOpen: () => void
}>(function TaskCard({ task, title, excerpt, assignee, unassignedLabel, timeLabel, sourceLabel, dragging, draggable = true, highlighted, onDragStart, onDragEnd, onOpen }, ref) {
  return (
    <button
      ref={ref}
      type="button"
      className={css.card}
      data-plugin="dsh-chaos"
      data-surface="kanban-task-card"
      data-status={task.status}
      data-dragging={dragging ? 'true' : undefined}
      data-highlighted={highlighted ? 'true' : undefined}
      aria-haspopup="dialog"
      draggable={draggable}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onClick={onOpen}
    >
      <span className={css.cardTop}>
        <span className={css.cardNumber}>#{task.number}</span>
        <span className={css.cardTime}>{timeLabel}</span>
      </span>
      <span className={css.cardTitle}>{title}</span>
      {excerpt !== '' && <span className={css.cardExcerpt}>{excerpt}</span>}
      <span className={css.cardFooter}>
        <span className={css.cardAssignee}>
          {assignee !== undefined && <AvatarChip kind="agent" handle={assignee.handle} displayName={assignee.displayName} avatarUrl={assignee.avatarDataUrl} />}
          <span>{assignee === undefined ? unassignedLabel : `@${assignee.handle}`}</span>
        </span>
        <span className={css.cardSource} title={sourceLabel} aria-hidden="true">
          <IconLinkOutline14 size={14} />
        </span>
      </span>
    </button>
  )
})
