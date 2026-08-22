import type { JSX } from 'react'
import { Button, IconLinkOutline16, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeTask } from '../../../native.ts'
import { AssigneePopover } from './AssigneePopover.tsx'
import css from './TaskBoard.module.css'
import type { ChaosTranslate } from '../../locales.ts'
import type { TaskStatus } from './task-model.ts'
import { TaskStatusDropdown } from './TaskStatusDropdown.tsx'
import { ErrorBanner } from '../../shared/ui/ErrorBanner.tsx'

export function TaskDetailDialog({ task, title, description, assigneeLabel, createdByLabel, selfActor, agents, error, t, onMove, onClaim, onUnclaim, onOpenAnchor, onClose }: {
  task: NativeTask
  title: string
  description: string
  assigneeLabel: string | undefined
  createdByLabel: string
  selfActor: NativeActor | undefined
  agents: NativeActor[]
  error: string | undefined
  t: ChaosTranslate
  onMove(target: TaskStatus): void
  onClaim(actorId?: string): void
  onUnclaim(): void
  onOpenAnchor(): void
  onClose(): void
}): JSX.Element {
  return (
    <Modal
      open
      onClose={onClose}
      title={t('tasks.detailTitle', { number: task.number })}
      closeLabel={t('members.close')}
      className={css.taskDetailDialog as string}
      contentClassName={css.taskDetailDialogContent as string}
    >
      <div className={css.taskDetailContent}>
        <h3>{title}</h3>
        {description !== '' && <p className={css.taskDetailDescription}>{description}</p>}
        <Button variant="outline" size="sm" icon={<IconLinkOutline16 size={16} />} onClick={onOpenAnchor}>
          {t('tasks.anchorGo')}
        </Button>
      </div>
      <div className={css.taskDetailDivider} aria-hidden="true" />
      {error !== undefined && <ErrorBanner>{error}</ErrorBanner>}
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
        <div className={css.taskDetailRow}><dt>{t('tasks.updated')}</dt><dd>{new Date(task.updatedAtMs).toLocaleString()}</dd></div>
      </dl>
    </Modal>
  )
}
