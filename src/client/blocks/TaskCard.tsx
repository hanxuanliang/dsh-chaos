/**
 * TaskCard — 看板任务卡。oil-creator 纸: 卡片根自带 data-plugin/data-surface
 * 标签, 规则 `.card[data-plugin='dsh-chaos']` 同为同元素的 0-2-0 特异性,
 * 宿主的 `.panel button{font:inherit}` 再也不到 .card 头上。
 * 原 classes: taskCard/taskCardTitle/taskCardExcerpt/taskCardMeta/
 *             taskCardNumber/taskCardAssignee/taskCardTime (独有 taskCardTime 的
 *             margin-left:auto + 性指定使分解)。
 */
import { forwardRef } from 'react'
import type { NativeTask } from '../../native.ts'
import css from './TaskCard.module.css'

export const TaskCard = forwardRef<HTMLButtonElement, {
  task: NativeTask
  title: string
  excerpt: string
  assigneeLabel: string | undefined
  unassignedLabel: string
  timeLabel: string
  dragging: boolean
  selected: boolean
  onDragStart: () => void
  onDragEnd: () => void
  onOpen: () => void
}>(function TaskCard({ task, title, excerpt, assigneeLabel, unassignedLabel, timeLabel, dragging, selected, onDragStart, onDragEnd, onOpen }, ref) {
  return (
    <button
      ref={ref}
      type="button"
      className={css.card}
      data-plugin="dsh-chaos"
      data-surface="kanban-task-card"
      data-status={task.status}
      data-dragging={dragging ? 'true' : undefined}
      data-selected={selected ? 'true' : undefined}
      aria-current={selected ? 'true' : undefined}
      aria-haspopup="dialog"
      draggable
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onClick={onOpen}
    >
      <span className={css.cardTitle}>{title}</span>
      {excerpt !== '' && <span className={css.cardExcerpt}>{excerpt}</span>}
      <span className={css.cardMeta}>
        <span className={css.cardNumber}>#{task.number}</span>
        <span className={css.cardAssignee}>{assigneeLabel ?? unassignedLabel}</span>
        <span className={css.cardTime}>{timeLabel}</span>
      </span>
    </button>
  )
})
