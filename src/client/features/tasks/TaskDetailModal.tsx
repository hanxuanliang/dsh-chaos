import type { JSX } from 'react'
import { Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeTask } from '../../../native.ts'
import { AssigneePopover } from './AssigneePopover.tsx'
import css from './TaskBoard.module.css'
import type { ChaosTranslate } from '../../locales.ts'
import { formatTaskTime, type TaskStatus } from './task-model.ts'
import { TaskStatusDropdown } from './TaskStatusDropdown.tsx'

export function TaskDetailModal({ task, title, assigneeLabel, createdByLabel, selfActor, agents, t, onMove, onClaim, onUnclaim, onOpenAnchor, onClose }: {
  task: NativeTask
  title: string
  assigneeLabel: string | undefined
  createdByLabel: string
  selfActor: NativeActor | undefined
  agents: NativeActor[]
  t: ChaosTranslate
  onMove(target: TaskStatus): void
  onClaim(actorId?: string): void
  onUnclaim(): void
  onOpenAnchor(): void
  onClose(): void
}): JSX.Element {
  return (
    <Modal open onClose={onClose} title={t('tasks.detailTitle', { number: task.number })} closeLabel={t('members.close')} contentClassName={css.dialogBody as string}>
      <div className={css.taskDetailTitleRow}>
        <button type="button" className={css.taskDetailTitleLink} title={t('tasks.anchorGo')} onClick={onOpenAnchor}>
          <span className={css.taskDetailTitle}>{title}</span>
        </button>
      </div>
      <dl className={css.taskDetailMeta}>
        <div className={css.taskDetailRow}>
          <dt>{t('tasks.status')}</dt>
          <dd><TaskStatusDropdown task={task} t={t} onMove={onMove} /></dd>
        </div>
        <div className={css.taskDetailRow}>
          <dt>{t('tasks.assignee')}</dt>
          <dd><AssigneePopover task={task} selfActor={selfActor} agents={agents} assigneeLabel={assigneeLabel} t={t} onClaim={onClaim} onUnclaim={onUnclaim} /></dd>
        </div>
        <div className={css.taskDetailRow}><dt>{t('tasks.sourceAuthor')}</dt><dd>{createdByLabel}</dd></div>
        <div className={css.taskDetailRow}><dt>{t('tasks.created')}</dt><dd>{new Date(task.createdAtMs).toLocaleString()}</dd></div>
        <div className={css.taskDetailRow}><dt>{t('tasks.updated')}</dt><dd>{formatTaskTime(task.updatedAtMs, t)}</dd></div>
      </dl>
    </Modal>
  )
}
