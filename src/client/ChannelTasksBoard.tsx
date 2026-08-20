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
import { IconCheckOutline16, IconChevronDownOutline14, IconChevronRightOutline14, IconUserOutline16, Menu, Modal } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeTask } from '../native.ts'
import type { ChaosKey, ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import css from './CollabPanel.module.css'
import { StatusChip } from './atoms/StatusChip.tsx'
import { TaskCard } from './blocks/TaskCard.tsx'
import { KanbanLane, KanbanLaneGrid } from './blocks/KanbanLane.tsx'
import { AssigneePopover } from './blocks/AssigneePopover.tsx'

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

/** Raft task panel idiom: small pencil = editable affordance cue. */

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
      <StatusChip
        status={task.status}
        label={t(LANE_LABEL_KEY[task.status])}
        title={t('tasks.statusChange')}
        onClick={() => { setOpen(v => !v) }}
      />
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
                  <IconCheckOutline16 />
                )}
              </button>
            )
          })}
        </span>
      )}
    </span>
  )
}

/** plocal assignee pill → 宿主 Menu + Pill: 重写(P0-4.1); 原自手 dropdown
 * 被拍"非常丑"——直接借鉴 pilot 原语(anchor=trigger pill, portal 开局不被
 * overflow 裁, selectedId 自带对勾)。 */
function AssigneeFilter({ t, members, value, onChange }: {
  t: ChaosTranslate
  members: NativeActor[]
  value: string
  onChange: (value: string) => void
}): JSX.Element {
  const [open, setOpen] = useState(false)

  const label = value === ''
    ? t('tasks.filterAssignee')
    : value === 'unassigned'
      ? t('tasks.unassigned')
      : `@${members.find(m => m.id === value)?.handle ?? '?'}`
  const selectedId = value === '' ? 'all' : value

  return (
    <Menu
      open={open}
      portal
      compact
      dense
      onClose={() => { setOpen(false) }}
      onSelect={(id) => { onChange(id === 'all' ? '' : id); setOpen(false) }}
      selectedId={selectedId}
      items={[
        { id: 'all', label: t('tasks.filterAll') },
        { id: 'unassigned', label: t('tasks.unassigned') },
        { type: 'separator', id: 'sep' },
        ...members.map(m => ({ id: m.id, label: `@${m.handle}` })),
      ]}
      anchor={
        <button
          type="button"
          className={`${css.filterPill} ${value !== '' ? css.filterPillActive : ''}`}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => { setOpen(v => !v) }}
        >
          <IconUserOutline16 aria-hidden="true" />
          {label}
          <IconChevronDownOutline14 />
        </button>
      }
    />
  )
}


function TaskDetailModal({ task, title, assigneeLabel, createdByLabel, selfActor, agents, t, onMove, onClaim, onUnclaim, onOpenAnchor, onClose }: {
  task: NativeTask
  title: string
  assigneeLabel: string | undefined
  createdByLabel: string
  selfActor: NativeActor | undefined
  agents: NativeActor[]
  t: ChaosTranslate
  onMove: (target: TaskStatus) => void
  onClaim: (actorId?: string) => void
  onUnclaim: () => void
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
        {/* 用户拍板：不需要独立锚定区——title 本身就是跳转链。 */}
        <button type="button" className={css.taskDetailTitleLink} title={t('tasks.anchorGo')} onClick={onOpenAnchor}>
          <span className={css.taskDetailTitle}>{title}</span>
          <IconChevronRightOutline14 />
        </button>
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
          <dd>
            <AssigneePopover
              task={task}
              selfActor={selfActor}
              agents={agents}
              assigneeLabel={assigneeLabel}
              t={t}
              onClaim={onClaim}
              onUnclaim={onUnclaim}
            />
          </dd>
        </div>
        <div className={css.taskDetailRow}>
          {/* Native Task has no creator column (pool semantics); the honest
              provenance is the anchor message's author. */}
          <dt>{t('tasks.sourceAuthor')}</dt>
          <dd>{createdByLabel}</dd>
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
  /** HTML5 dnd (plocal dnd-kit 的最小依赖同义实现): 拖一张 task 卡, 列只在状态机可达时点亮。 */
  const [draggingTaskId, setDraggingTaskId] = useState<string | undefined>(undefined)
  const [dragOverLane, setDragOverLane] = useState<TaskStatus | undefined>(undefined)

  const tasks = useMemo(() => {
    const list = Object.values(state.tasksByMessage).filter(task => task.targetId === channelId)
    const visible = assigneeFilter === ''
      ? list
      : list.filter(task => assigneeFilter === 'unassigned'
        ? task.assigneeId === undefined
        : task.assigneeId === assigneeFilter)
    return visible.sort((a, b) => b.updatedAtMs - a.updatedAtMs)
  }, [state.tasksByMessage, channelId, assigneeFilter])

  // 用户拍板：认领人筛选只列 channel 内 agents（人侧认领面不放进筛选 pill）。
  const members = (state.membersByChannel[channelId] ?? []).filter(m => m.kind === 'agent')

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

  const createdByLabelOf = (task: NativeTask): string => {
    const msg = state.messagesByChannel[channelId]?.find(m => m.id === task.messageId)
    if (msg === undefined) return t('tasks.authorUnknown')
    const actor = state.actors.find(a => a.id === msg.authorId)
    return actor === undefined ? t('tasks.authorUnknown') : `@${actor.handle}`
  }

  const surfaceError = (error: unknown): void => {
    setMoveError(t('tasks.statusFailed', { error: error instanceof Error ? error.message : String(error) }))
  }

  const move = (task: NativeTask, target: TaskStatus): void => {
    setMoveError(undefined)
    // todo(unassigned) → in_progress routes through claim so the move
    // attaches an assignee (spec §3.1); everything else is a direct update.
    const action = task.status === 'todo' && target === 'in_progress' && task.assigneeId === undefined
      ? store.claimTask(task.messageId)
      : store.updateTaskStatus(task.messageId, target)
    action.catch(surfaceError)
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
      <KanbanLaneGrid>
        {LANES.map((lane) => {
          const laneTasks = tasks.filter(task => task.status === lane)
          const draggingTask = draggingTaskId === undefined ? undefined : tasks.find(t => t.messageId === draggingTaskId)
          const laneAcceptsDrag = draggingTask !== undefined && draggingTask.status !== lane
            && ALLOWED[draggingTask.status].includes(lane)
          return (
            <KanbanLane
              key={lane}
              status={lane}
              label={t(LANE_LABEL_KEY[lane])}
              count={laneTasks.length}
              dragOver={dragOverLane === lane && laneAcceptsDrag}
              emptyLabel={t('tasks.laneEmpty')}
              onDragOver={(event) => {
                if (!laneAcceptsDrag) return
                event.preventDefault()
                setDragOverLane(lane)
              }}
              onDragLeave={() => { setDragOverLane(undefined) }}
              onDrop={(event) => {
                event.preventDefault()
                setDragOverLane(undefined)
                if (draggingTask === undefined) return
                setDraggingTaskId(undefined)
                move(draggingTask, lane)
              }}
            >
              {laneTasks.map((task) => {
                const { title, excerpt } = splitAnchor(anchorOf(task))
                return (
                  <TaskCard
                    key={task.messageId}
                    task={task}
                    title={title === '' ? `#${task.number}` : title}
                    excerpt={excerpt}
                    assigneeLabel={assigneeLabelOf(task)}
                    unassignedLabel={t('tasks.unassigned')}
                    timeLabel={formatTime(task.updatedAtMs, t)}
                    dragging={draggingTaskId === task.messageId}
                    onDragStart={() => { setDraggingTaskId(task.messageId) }}
                    onDragEnd={() => { setDraggingTaskId(undefined); setDragOverLane(undefined) }}
                    onOpen={() => { setMoveError(undefined); setSelectedMessageId(task.messageId) }}
                  />
                )
              })}
            </KanbanLane>
          )
        })}
      </KanbanLaneGrid>
      {selected !== undefined && (
        <TaskDetailModal
          task={selected}
          title={(() => { const s = splitAnchor(anchorOf(selected)); return s.title === '' ? `#${selected.number}` : s.title })()}
          assigneeLabel={assigneeLabelOf(selected)}
          createdByLabel={createdByLabelOf(selected)}
          selfActor={state.actors.find(a => a.id === state.selfId)}
          agents={members}
          t={t}
          onMove={(target) => { move(selected, target) }}
          onClaim={(actorId) => { setMoveError(undefined); store.claimTask(selected.messageId, actorId).catch(surfaceError) }}
          onUnclaim={() => { setMoveError(undefined); store.unclaimTask(selected.messageId).catch(surfaceError) }}
          onOpenAnchor={() => { setSelectedMessageId(undefined); onOpenMessage(selected.messageId) }}
          onClose={() => { setSelectedMessageId(undefined) }}
        />
      )}
    </div>
  )
}
