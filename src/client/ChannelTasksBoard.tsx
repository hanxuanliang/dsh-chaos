/**
 * Channel tasks board (P0-4).
 *
 * Truth sources:
 * - Layout/skin anatomy: zrepo/dsh-web-ui packages/dsh-task-board (columns =
 *   grid-auto-flow:column minmax(220px,1fr), column = bg-layer-2 + l1 + r12,
 *   statusDot + 13/700 title + count pill, card = bg-base + l2 + r10 with
 *   hover shadow-lv2 + lift, per-column empty copy). Five-dim mapping logged
 *   in docs/frontend-style-guide.md §P0-4.
 * - Filter: plocal-web ChannelTasksBoard — the toolbar filter is an
 *   ASSIGNEE pill dropdown ("Assignee ▾" with All / Unassigned / members),
 *   not a free-text search (user feedback 2026-08-19).
 * - Status control: plocal-web TaskStatusMenu — a tinted status chip opening a
 *   menu of statuses; rows whose transition is not legal render disabled
 *   instead of pretending to work. Legal transitions mirror the backend truth
 *   in crates/collab-core task_transition_allowed:
 *     todo → in_progress
 *     in_progress → todo | in_review
 *     in_review → in_progress | done
 *     done → in_progress   (reopen)
 *   Special case: todo(unassigned) → in_progress goes through task.claim so
   * the move attaches an assignee (spec §3.1's drop-to-progress rule).
 * - Detail: spec §3.2 — centered Modal; the status chip dropdown lives inside
 *   the meta rows (never leading the head, because it is now an editor); the
 *   anchor message is a LINK: clicking closes the modal, switches to the
 *   Messages tab, and scroll-flashes the anchor row.
 * - Data truth: NativeTask is message-anchored; tasksByMessage refetches on
 *   SSE task_created/task_updated, so the board is a pure projection.
 */
import { useEffect, useMemo, useRef, useState, type JSX } from 'react'
import { Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeTask } from '../native.ts'
import type { ChaosKey, ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import css from './CollabPanel.module.css'

type TaskStatus = NativeTask['status']

/** Spec §3.1 lanes, order locked: 待办 → 进行 → 评审 → 完成. */
const LANES: readonly TaskStatus[] = ['todo', 'in_progress', 'in_review', 'done']

const LANE_LABEL_KEY: Record<TaskStatus, ChaosKey> = {
  todo: 'tasks.lane.todo',
  in_progress: 'tasks.lane.inProgress',
  in_review: 'tasks.lane.inReview',
  done: 'tasks.lane.done',
}

/** crates/collab-core task_transition_allowed — keep in lockstep. */
const ALLOWED: Record<TaskStatus, readonly TaskStatus[]> = {
  todo: ['in_progress'],
  in_progress: ['todo', 'in_review'],
  in_review: ['in_progress', 'done'],
  done: ['in_progress'],
}

/** Compact relative/absolute time label (dsh-task-board TaskCard.formatTime). */
function formatTime(ms: number, t: ChaosTranslate): string {
  const minutes = Math.floor((Date.now() - ms) / 60000)
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

/**
 * plocal TaskStatusMenu: tinted chip → status menu. Non-reachable rows are
 * disabled, never fake-clickable. Anchors inline (parent is positioned) —
 * plocal portals only because its menus live inside virtualized rows.
 */
function TaskStatusDropdown({ task, t, onMove }: {
  task: NativeTask
  t: ChaosTranslate
  onMove: (target: TaskStatus) => void
}): JSX.Element {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent): void => {
      if (rootRef.current?.contains(event.target as Node) !== true) setOpen(false)
    }
    const closeKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', closeKey)
    return () => {
      document.removeEventListener('mousedown', close)
      document.removeEventListener('keydown', closeKey)
    }
  }, [open])

  const reachable = ALLOWED[task.status]
  return (
    <span ref={rootRef} className={css.statusDropdown}>
      <button
        type="button"
        className={css.statusChip}
        data-status={task.status}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t('tasks.statusChange')}
        onClick={() => { setOpen(v => !v) }}
      >
        <span className={css.statusDot} data-status={task.status} aria-hidden="true" />
        {t(LANE_LABEL_KEY[task.status])}
        <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="m4.5 6.5 3.5 3.5 3.5-3.5" />
        </svg>
      </button>
      {open && (
        <span role="menu" className={css.statusMenu}>
          {LANES.map((lane) => {
            const legal = reachable.includes(lane)
            return (
              <button
                key={lane}
                type="button"
                role="menuitem"
                className={css.statusMenuItem}
                disabled={!legal}
                title={legal ? undefined : t('tasks.statusIllegal')}
                onClick={() => { setOpen(false); onMove(lane) }}
              >
                <span className={css.statusDot} data-status={lane} aria-hidden="true" />
                <span className={css.statusMenuLabel}>{t(LANE_LABEL_KEY[lane])}</span>
                {lane === task.status && (
                  <svg viewBox="0 0 16 16" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="m3.5 8.5 3 3 6-7" />
                  </svg>
                )}
              </button>
            )
          })}
        </span>
      )}
    </span>
  )
}

/** plocal assignee pill: All / Unassigned / channel members. */
function AssigneeFilter({ t, members, value, onChange }: {
  t: ChaosTranslate
  members: NativeActor[]
  value: string
  onChange: (value: string) => void
}): JSX.Element {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLSpanElement>(null)

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent): void => {
      if (rootRef.current?.contains(event.target as Node) !== true) setOpen(false)
    }
    document.addEventListener('mousedown', close)
    return () => { document.removeEventListener('mousedown', close) }
  }, [open])

  const label = value === ''
    ? t('tasks.filterAssignee')
    : value === 'unassigned'
      ? t('tasks.unassigned')
      : `@${members.find(m => m.id === value)?.handle ?? '?'}`
  const item = (key: string, text: string): JSX.Element => (
    <button
      key={key === '' ? 'all' : key}
      type="button"
      role="menuitem"
      className={css.statusMenuItem}
      onClick={() => { onChange(key); setOpen(false) }}
    >
      <span className={css.statusMenuLabel}>{text}</span>
      {value === key && (
        <svg viewBox="0 0 16 16" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="m3.5 8.5 3 3 6-7" />
        </svg>
      )}
    </button>
  )

  return (
    <span ref={rootRef} className={css.statusDropdown}>
      <button
        type="button"
        className={`${css.filterPill} ${value !== '' ? css.filterPillActive : ''}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => { setOpen(v => !v) }}
      >
        <svg viewBox="0 0 16 16" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="8" cy="5.5" r="2.5" />
          <path d="M3.5 13.5c.8-2.2 2.5-3.2 4.5-3.2s3.7 1 4.5 3.2" />
        </svg>
        {label}
        <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="m4.5 6.5 3.5 3.5 3.5-3.5" />
        </svg>
      </button>
      {open && (
        <span role="menu" className={css.statusMenu}>
          {item('', t('tasks.filterAll'))}
          {item('unassigned', t('tasks.unassigned'))}
          {members.map(m => item(m.id, `@${m.handle}`))}
        </span>
      )}
    </span>
  )
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
        <span className={css.taskCardAssignee}>{assigneeLabel ?? t('tasks.unassigned')}</span>
        <span className={css.taskCardTime}>{formatTime(task.updatedAtMs, t)}</span>
      </span>
    </button>
  )
}

function TaskDetailModal({ task, title, anchor, assigneeLabel, t, onMove, onOpenAnchor, onClose }: {
  task: NativeTask
  title: string
  anchor: string
  assigneeLabel: string | undefined
  t: ChaosTranslate
  onMove: (target: TaskStatus) => void
  onOpenAnchor: () => void
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
      <div className={css.taskDetailTitleRow}>
        <span className={css.taskDetailTitle}>{title}</span>
      </div>
      <dl className={css.taskDetailMeta}>
        <div className={css.taskDetailRow}>
          <dt>{t('tasks.status')}</dt>
          <dd>
            <TaskStatusDropdown task={task} t={t} onMove={onMove} />
          </dd>
        </div>
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
        {/* Anchor = a jump link, never a dead quote: click lands on the
            source message in the stream (user direction 2026-08-19). */}
        <button type="button" className={css.taskDetailAnchorLink} onClick={onOpenAnchor}>
          <span className={css.taskDetailAnchorText}>{anchor}</span>
          <span className={css.taskDetailAnchorGo}>
            {t('tasks.anchorGo')}
            <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M3 8h9M8.5 4 12.5 8 8.5 12" />
            </svg>
          </span>
        </button>
      </div>
    </Modal>
  )
}

export function ChannelTasksBoard({ t, store, state, channelId, onOpenMessage }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channelId: string
  /** Close modal + switch to messages + scroll-flash the anchor row. */
  onOpenMessage: (messageId: string) => void
}): JSX.Element {
  const [assigneeFilter, setAssigneeFilter] = useState('')
  const [selectedMessageId, setSelectedMessageId] = useState<string | undefined>(undefined)
  const [moveError, setMoveError] = useState<string | undefined>(undefined)

  const tasks = useMemo(() => {
    const list = Object.values(state.tasksByMessage).filter(task => task.targetId === channelId)
    const visible = assigneeFilter === ''
      ? list
      : list.filter(task => assigneeFilter === 'unassigned'
        ? task.assigneeId === undefined
        : task.assigneeId === assigneeFilter)
    return visible.sort((a, b) => b.updatedAtMs - a.updatedAtMs)
  }, [state.tasksByMessage, channelId, assigneeFilter])

  const members = state.membersByChannel[channelId] ?? []

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

  const move = (task: NativeTask, target: TaskStatus): void => {
    setMoveError(undefined)
    // todo(unassigned) → in_progress routes through claim so the move
    // attaches an assignee (spec §3.1); everything else is a direct update.
    const action = task.status === 'todo' && target === 'in_progress' && task.assigneeId === undefined
      ? store.claimTask(task.messageId)
      : store.updateTaskStatus(task.messageId, target)
    action.catch((error: unknown) => {
      setMoveError(t('tasks.statusFailed', { error: error instanceof Error ? error.message : String(error) }))
    })
  }

  const selected = selectedMessageId === undefined
    ? undefined
    : state.tasksByMessage[selectedMessageId]

  return (
    <div className={css.taskBoard}>
      <header className={css.taskBoardHeader}>
        <AssigneeFilter t={t} members={members} value={assigneeFilter} onChange={setAssigneeFilter} />
      </header>
      {moveError !== undefined && (
        <div className={css.taskBoardError} role="alert">{moveError}</div>
      )}
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
                      onOpen={() => { setMoveError(undefined); setSelectedMessageId(task.messageId) }}
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
          onMove={(target) => { move(selected, target) }}
          onOpenAnchor={() => { setSelectedMessageId(undefined); onOpenMessage(selected.messageId) }}
          onClose={() => { setSelectedMessageId(undefined) }}
        />
      )}
    </div>
  )
}
