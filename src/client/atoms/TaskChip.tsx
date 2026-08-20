/**
 * TaskChip — 流卡/活动卡通用的 task 小 chip （原 MessageRow 内嵌):
 * `[icon] #N @assignee` 20px pill, --chip 分态色(todo 黄 / in_progress 蓝 /
 * in_review 紫 / done 绿), chip 底 = --chip 15% tint。
 * なく shek、用、这一件筩 Task 摘要本体的原味原件。
 */
import type { NativeTask } from '../../native.ts'
import css from './TaskChip.module.css'

function StatusIcon({ status }: { status: NativeTask['status'] }): JSX.Element {
  return (
    <svg viewBox="0 0 16 16" width="9" height="9" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className={css.statusIcon} data-status={status}>
      <circle cx="8" cy="8" r="5.5" />
    </svg>
  )
}

export function TaskChip({ task, assignee, onClick }: {
  task: Pick<NativeTask, 'number' | 'status'>
  assignee: string | undefined
  onClick(): void
}): JSX.Element {
  return (
    <button type="button" className={css.chip} data-plugin="dsh-chaos" data-status={task.status} onClick={onClick}>
      <StatusIcon status={task.status} />
      <span className={css.chipId}>#{task.number}</span>
      {assignee !== undefined && <span className={css.chipAssignee}>@{assignee}</span>}
    </button>
  )
}
