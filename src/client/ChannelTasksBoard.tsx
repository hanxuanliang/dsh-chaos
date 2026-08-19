/**
 * Channel tasks board (P0-4, read-only v1).
 *
 * Truth sources:
 * - Layout/skin anatomy: zrepo/dsh-web-ui packages/dsh-task-board (columns =
 *   grid-auto-flow:column minmax(220px,1fr), column = bg-layer-2 + l1 + r12,
 *   statusDot + 13/700 title + count pill, card = bg-base + l2 + r10 with
 *   hover shadow-lv2 + lift, per-column empty copy). Full five-dim mapping in
 *   docs/frontend-style-guide.md progress log.
 * - Interaction contract: docs/dsh-chaos-interaction-spec.md §3.1 — 4 lanes
 *   and the task detail as a centered Modal; plocal-web/ChannelTasks? (not in
 *   plocal) — spec is authoritative.
 * - Data truth: NativeTask is message-anchored (messageId + number + version);
 *   tasks arrive via collab-store.tasksByMessage and refresh on every SSE
 *   task_created/task_updated invalidation — this board is a pure projection.
 *
 * Scope lock (user decision 2026-08-19): humans do NOT create/claim/move
 * tasks here — agents create from messages and flip their own status. There
 * is no "+ new task" affordance, no drag, no claim button in this version.
 */
import { useMemo, useState, type JSX } from 'react'
import { Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTask } from '../native.ts'
import type { ChaosKey, ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import css from './CollabPanel.module.css'

type TaskStatus = NativeTask['status']

/** Spec §3.1 lanes, order locked: 待办 → 进行 → 评审 → 完成. */
const LANES: readonly TaskStatus[] = ['todo', 'in_progress', 'in_review', 'done']

/** dsh-task-board/board.module.css .statusDot[data-status] ramp. */
const LANE_LABEL_KEY: Record<TaskStatus, ChaosKey> = {
  todo: 'tasks.lane.todo',
  in_progress: 'tasks.lane.inProgress',
  in_review: 'tasks.lane.inReview',
  done: 'tasks.lane.done',
}

/** Compact relative/absolute time label (dsh-task-board TaskCard.formatTime). */
function formatTime(ms: number, t: ChaosTranslate): string {
  const now = Date.now()
  const minutes = Math.floor((now - ms) / 60000)
  if (minutes < 1) return t('tasks.time.justNow')
  if (minutes < 60) return `${minutes}m`
  if (minutes < 60 * 24) return `${Math.floor(minutes / 60)}h`
  const date = new Date(ms)
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
}

/** First line as card title; the rest (if any) as the excerpt. */
function splitAnchor(text: string): { title: string; excerpt: string } {
  const trimmed = text.trim()
  const cut = trimmed.indexOf('\n')
  if (cut < 0) return { title: trimmed, excerpt: '' }
  return { title: trimmed.slice(0, cut), excerpt: trimmed.slice(cut + 1).trim() }
}

function TaskCard({ task, title, excerpt, assigneeLabel, t, onOpen }: {
  task: NativeTask
  title: string
  excerpt: string
  assigneeLabel: string | undefined
  t: ChaosTranslate
  onOpen: () => void
}): JSX.Element {
  return (
    <button type="button" className={css.taskCard} data-status={task.status} onClick={onOpen}>
      <span className={css.taskCardTitle}>{title}</span>
      {excerpt !== '' && <span className={css.taskCardExcerpt}>{excerpt}</span>}
      <span className={css.taskCardMeta}>
        <span className={css.taskCardNumber}>#{task.number}</span>
        {assigneeLabel !== undefined && <span className={css.taskCardAssignee}>{assigneeLabel}</span>}
        <span className={css.taskCardTime}>{formatTime(task.updatedAtMs, t)}</span>
      </span>
    </button>
  )
}

function TaskDetailModal({ task, title, anchor, assigneeLabel, t, onClose }: {
  task: NativeTask
  title: string
  anchor: string
  assigneeLabel: string | undefined
  t: ChaosTranslate
  onClose: () => void
}): JSX.Element {
  return (
    <Modal
      open
      onClose={onClose}
      title={t('tasks.detailTitle', { number: task.number })}
      closeLabel={t('members.close')}
      contentClassName={css.dialogBody as string}
    >
      <div className={css.taskDetailHead}>
        <span className={css.statusDot} data-status={task.status} aria-hidden="true" />
        <span className={css.taskDetailStatus}>{t(LANE_LABEL_KEY[task.status])}</span>
        <span className={css.taskDetailTitle}>{title}</span>
      </div>
      <dl className={css.taskDetailMeta}>
        <div className={css.taskDetailRow}>
          <dt>{t('tasks.assignee')}</dt>
          <dd>{assigneeLabel ?? t('tasks.unassigned')}</dd>
        </div>
        <div className={css.taskDetailRow}>
          <dt>{t('tasks.created')}</dt>
          <dd>{new Date(task.createdAtMs).toLocaleString()}</dd>
        </div>
        <div className={css.taskDetailRow}>
          <dt>{t('tasks.updated')}</dt>
          <dd>{formatTime(task.updatedAtMs, t)}</dd>
        </div>
      </dl>
      <div className={css.taskDetailAnchor}>
        <div className={css.taskDetailAnchorLabel}>{t('tasks.anchor')}</div>
        <div className={css.taskDetailAnchorText}>{anchor}</div>
      </div>
    </Modal>
  )
}

export function ChannelTasksBoard({ t, store, state, channelId }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channelId: string
}): JSX.Element {
  const [filter, setFilter] = useState('')
  const [selectedMessageId, setSelectedMessageId] = useState<string | undefined>(undefined)
  void store

  const tasks = useMemo(() => {
    const list = Object.values(state.tasksByMessage).filter(task => task.targetId === channelId)
    const needle = filter.trim().toLowerCase()
    const visible = needle === ''
      ? list
      : list.filter((task) => {
        const text = (task.anchorText ?? '').toLowerCase()
        return text.includes(needle) || task.number.toLowerCase().includes(needle)
      })
    return visible.sort((a, b) => b.updatedAtMs - a.updatedAtMs)
  }, [state.tasksByMessage, channelId, filter])

  const assigneeLabelOf = (task: NativeTask): string | undefined => {
    if (task.assigneeId === undefined) return undefined
    const actor = state.actors.find(a => a.id === task.assigneeId)
    return actor === undefined ? undefined : `@${actor.handle}`
  }

  const anchorOf = (task: NativeTask): string => {
    if (task.anchorText !== undefined && task.anchorText.trim() !== '') return task.anchorText
    const msg = state.messagesByChannel[channelId]?.find(m => m.id === task.messageId)
    if (msg !== undefined) return msg.text
    return t('tasks.anchorMissing')
  }

  const selected = selectedMessageId === undefined
    ? undefined
    : state.tasksByMessage[selectedMessageId]

  return (
    <div className={css.taskBoard}>
      <header className={css.taskBoardHeader}>
        <input
          className={css.taskBoardSearch}
          type="search"
          value={filter}
          placeholder={t('tasks.search')}
          aria-label={t('tasks.search')}
          onChange={(e) => { setFilter(e.target.value) }}
        />
      </header>
      <div className={css.taskColumns}>
        {LANES.map((lane) => {
          const laneTasks = tasks.filter(task => task.status === lane)
          return (
            <section key={lane} className={css.taskColumn} data-status={lane}>
              <header className={css.taskColumnHeader}>
                <span className={css.statusDot} data-status={lane} aria-hidden="true" />
                <h3 className={css.taskColumnTitle}>{t(LANE_LABEL_KEY[lane])}</h3>
                <span className={css.taskColumnCount}>{laneTasks.length}</span>
              </header>
              <div className={css.taskCards}>
                {laneTasks.map((task) => {
                  const { title, excerpt } = splitAnchor(anchorOf(task))
                  return (
                    <TaskCard
                      key={task.messageId}
                      task={task}
                      title={title === '' ? `#${task.number}` : title}
                      excerpt={excerpt}
                      assigneeLabel={assigneeLabelOf(task)}
                      t={t}
                      onOpen={() => { setSelectedMessageId(task.messageId) }}
                    />
                  )
                })}
                {laneTasks.length === 0 && (
                  <div className={css.taskColumnEmpty}>{t('tasks.laneEmpty')}</div>
                )}
              </div>
            </section>
          )
        })}
      </div>
      {selected !== undefined && (
        <TaskDetailModal
          task={selected}
          title={(() => { const s = splitAnchor(anchorOf(selected)); return s.title === '' ? `#${selected.number}` : s.title })()}
          anchor={anchorOf(selected)}
          assigneeLabel={assigneeLabelOf(selected)}
          t={t}
          onClose={() => { setSelectedMessageId(undefined) }}
        />
      )}
    </div>
  )
}
