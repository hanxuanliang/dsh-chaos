/**
 * Channel tasks board (P0-4).
 *
 * Contract:
 * - The board uses horizontally scrolling lanes, host design tokens, and one
 *   compact card per authoritative task projection.
 * - The toolbar filters by assignee (All / Unassigned / current members).
 * - A tinted status chip opens the transition menu; illegal transitions stay
 *   visible but disabled. Legal transitions mirror the backend truth in
 *   crates/collab-core task_transition_allowed:
 *     todo → in_progress
 *     in_progress → todo | in_review
 *     in_review → in_progress | done
 *     done → in_progress   (reopen)
 *   Special case: todo(unassigned) → in_progress goes through task.claim so
   * the move attaches an assignee (spec §3.1's drop-to-progress rule).
 * - Task info opens in a focused card dialog. A divider separates the source
 *   content from live status, assignee, author, and timestamp metadata. The
 *   source action opens Messages and highlights the anchor.
 * - Data truth: NativeTask is message-anchored; tasksByMessage refetches on
 *   SSE task_created/task_updated, so the board is a pure projection.
 */
import { useEffect, useMemo, useRef, useState, type JSX } from 'react'
import type { NativeTask } from '../../../native.ts'
import type { ChaosTranslate } from '../../locales.ts'
import type { CollabStore, CollabStoreSnapshot } from '../../data/store.ts'
import css from './TaskBoard.module.css'
import { TaskCard } from './TaskCard.tsx'
import { KanbanLane, KanbanLaneGrid } from './KanbanLane.tsx'
import { ErrorBanner } from '../../shared/ui/index.ts'
import { Toolbar } from '../../shared/layout/index.ts'
import { AssigneeFilter } from './AssigneeFilter.tsx'
import { TaskDetailDialog } from './TaskDetailDialog.tsx'
import { TASK_LANES, TASK_LANE_LABEL_KEY, TASK_TRANSITIONS, formatTaskTime, splitTaskAnchor, type TaskStatus } from './task-model.ts'

export function ChannelTasksBoard({ t, store, state, channelId, focusMessageId, readOnly = false, onFocusHandled, onOpenMessage }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channelId: string
  /** One-shot request from a message Task chip: reveal and focus its card. */
  focusMessageId: string | undefined
  readOnly?: boolean | undefined
  onFocusHandled: () => void
  /** Close modal + switch to messages + scroll-flash the anchor row. */
  onOpenMessage: (messageId: string) => void
}): JSX.Element {
  const [assigneeFilter, setAssigneeFilter] = useState('')
  const [highlightedMessageId, setHighlightedMessageId] = useState<string | undefined>(undefined)
  const [selectedMessageId, setSelectedMessageId] = useState<string | undefined>(undefined)
  const [moveError, setMoveError] = useState<string | undefined>(undefined)
  /** HTML5 dnd (plocal dnd-kit 的最小依赖同义实现): 拖一张 task 卡, 列只在状态机可达时点亮。 */
  const [draggingTaskId, setDraggingTaskId] = useState<string | undefined>(undefined)
  const [dragOverLane, setDragOverLane] = useState<TaskStatus | undefined>(undefined)
  const cardRefs = useRef(new Map<string, HTMLButtonElement>())
  const highlightTimerRef = useRef<number | undefined>(undefined)

  useEffect(() => () => {
    if (highlightTimerRef.current !== undefined) window.clearTimeout(highlightTimerRef.current)
  }, [])

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

  // Message → Task is an identity jump, not merely a tab switch. If the
  // current assignee filter hides the target, clear it first; then scroll the
  // exact card to the center, give it keyboard focus, and briefly mark it.
  useEffect(() => {
    if (focusMessageId === undefined) return
    const task = state.tasksByMessage[focusMessageId]
    if (task === undefined || task.targetId !== channelId) return
    const hiddenByFilter = assigneeFilter !== '' && (
      assigneeFilter === 'unassigned'
        ? task.assigneeId !== undefined
        : task.assigneeId !== assigneeFilter
    )
    if (hiddenByFilter) {
      setAssigneeFilter('')
      return
    }
    if (highlightTimerRef.current !== undefined) window.clearTimeout(highlightTimerRef.current)
    // Commit an off frame first so an immediate repeat to the same Task
    // restarts the CSS marker instead of inheriting its previous timeline.
    setHighlightedMessageId(undefined)
    const frame = window.requestAnimationFrame(() => {
      const card = cardRefs.current.get(focusMessageId)
      card?.scrollIntoView({ block: 'center', inline: 'nearest' })
      card?.focus({ preventScroll: true })
      setHighlightedMessageId(focusMessageId)
      highlightTimerRef.current = window.setTimeout(() => {
        setHighlightedMessageId(current => current === focusMessageId ? undefined : current)
        highlightTimerRef.current = undefined
      }, 2200)
      onFocusHandled()
    })
    return () => { window.cancelAnimationFrame(frame) }
  }, [focusMessageId, state.tasksByMessage, channelId, assigneeFilter, onFocusHandled])

  const assigneeOf = (task: NativeTask) => {
    if (task.assigneeId === undefined) return undefined
    return state.actors.find(a => a.id === task.assigneeId)
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
    if (readOnly) return
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

  const closeDetail = (): void => {
    const messageId = selectedMessageId
    setSelectedMessageId(undefined)
    if (messageId === undefined) return
    window.requestAnimationFrame(() => { cardRefs.current.get(messageId)?.focus() })
  }

  return (
    <div className={css.taskBoard}>
      <Toolbar className={css.taskBoardHeader} start={<AssigneeFilter t={t} members={members} value={assigneeFilter} onChange={setAssigneeFilter} />} />
      {moveError !== undefined && (
        <ErrorBanner>{moveError}</ErrorBanner>
      )}
      <KanbanLaneGrid>
        {TASK_LANES.map((lane) => {
          const laneTasks = tasks.filter(task => task.status === lane)
          const draggingTask = draggingTaskId === undefined ? undefined : tasks.find(t => t.messageId === draggingTaskId)
          const laneAcceptsDrag = !readOnly && draggingTask !== undefined && draggingTask.status !== lane
            && TASK_TRANSITIONS[draggingTask.status].includes(lane)
          return (
            <KanbanLane
              key={lane}
              status={lane}
              label={t(TASK_LANE_LABEL_KEY[lane])}
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
                const { title, excerpt } = splitTaskAnchor(anchorOf(task))
                return (
                  <TaskCard
                    key={task.messageId}
                    ref={(node) => {
                      if (node === null) cardRefs.current.delete(task.messageId)
                      else cardRefs.current.set(task.messageId, node)
                    }}
                    task={task}
                    title={title === '' ? `#${task.number}` : title}
                    excerpt={excerpt}
                    assignee={assigneeOf(task)}
                    unassignedLabel={t('tasks.unassigned')}
                    timeLabel={formatTaskTime(task.updatedAtMs, t)}
                    sourceLabel={t('tasks.anchorGo')}
                    dragging={!readOnly && draggingTaskId === task.messageId}
                    draggable={!readOnly}
                    highlighted={highlightedMessageId === task.messageId}
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
      {selected !== undefined && (() => {
        const anchor = splitTaskAnchor(anchorOf(selected))
        return (
          <TaskDetailDialog
            task={selected}
            title={anchor.title === '' ? `#${selected.number}` : anchor.title}
            description={anchor.excerpt}
            assigneeLabel={assigneeOf(selected) === undefined ? undefined : `@${assigneeOf(selected)?.handle ?? ''}`}
            createdByLabel={createdByLabelOf(selected)}
            selfActor={state.actors.find(a => a.id === state.selfId)}
            agents={members}
            error={moveError}
            readOnly={readOnly}
            t={t}
            onMove={(target) => { move(selected, target) }}
            onClaim={(actorId) => { setMoveError(undefined); store.claimTask(selected.messageId, actorId).catch(surfaceError) }}
            onUnclaim={() => { setMoveError(undefined); store.unclaimTask(selected.messageId).catch(surfaceError) }}
            onOpenAnchor={() => { setSelectedMessageId(undefined); onOpenMessage(selected.messageId) }}
            onClose={closeDetail}
          />
        )
      })()}
    </div>
  )
}
