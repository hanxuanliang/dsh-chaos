import type { JSX } from 'react'
import { Button, IconLinkOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeTask } from '../../../native.ts'
import { AssigneePopover } from './AssigneePopover.tsx'
import css from './TaskBoard.module.css'
import type { ChaosTranslate } from '../../locales.ts'
import type { TaskStatus } from './task-model.ts'
import { TaskStatusDropdown } from './TaskStatusDropdown.tsx'
import { PanelHeader } from '../../shared/layout/PanelHeader.tsx'
import { ErrorBanner } from '../../shared/ui/ErrorBanner.tsx'

export function TaskDetailView({ task, title, description, assigneeLabel, createdByLabel, selfActor, agents, error, t, onMove, onClaim, onUnclaim, onOpenAnchor, onBack }: {
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
  onBack(): void
}): JSX.Element {
  return (
    <section className={css.taskDetailSurface} aria-label={t('tasks.detailTitle', { number: task.number })}>
      <PanelHeader title={t('tasks.detailTitle', { number: task.number })} backLabel={t('tasks.back')} onBack={onBack} />
      {error !== undefined && <ErrorBanner className={css.taskDetailError}>{error}</ErrorBanner>}
      <div className={css.taskDetailLayout}>
        <div className={css.taskDetailMain}>
          <div className={css.taskDetailContent}>
            <p className={css.taskDetailEyebrow}>{t('tasks.anchor')}</p>
            <h1>{title}</h1>
            {description !== '' && <p className={css.taskDetailDescription}>{description}</p>}
            <Button variant="outline" size="sm" icon={<IconLinkOutline16 size={16} />} onClick={onOpenAnchor}>
              {t('tasks.anchorGo')}
            </Button>
          </div>
        </div>
        <aside className={css.taskDetailProperties} aria-label={t('tasks.properties')}>
          <h2>{t('tasks.properties')}</h2>
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
        </aside>
      </div>
    </section>
  )
}
