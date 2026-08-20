/**
 * TaskCard — 看板任务卡。oil-creator 纸: 卡片根自带 data-plugin/data-surface
 * 标签, 规则 `.card[data-plugin='dsh-chaos']` 同为同元素的 0-2-0 特异性,
 * 宿主的 `.panel button{font:inherit}` 再也不到 .card 头上。
 * 原 classes: taskCard/taskCardTitle/taskCardExcerpt/taskCardMeta/
 *             taskCardNumber/taskCardAssignee/taskCardTime (独有 taskCardTime 的
 *             margin-left:auto + 性指定使分解)。
 */
import type { NativeTask } from '../../native.ts'
import css from './TaskCard.module.css'

export function TaskCard({ task, title, excerpt, assigneeLabel, unassignedLabel, timeLabel, dragging, onDragStart, onDragEnd, onOpen }: {
  task: NativeTask
  title: string
  excerpt: string
  assigneeLabel: string | undefined
  unassignedLabel: string
  timeLabel: string
  dragging: boolean
  onDragStart: () => void
  onDragEnd: () => void
  onOpen: () => void
}) {
  return (
    <button
      type="button"
      className={css.card}
      data-plugin="dsh-chaos"
      data-surface="kanban-task-card"
      data-status={task.status}
      data-dragging={dragging ? 'true' : undefined}
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
}
