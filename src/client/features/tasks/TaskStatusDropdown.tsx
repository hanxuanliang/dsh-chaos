import { useEffect, useRef, useState, type JSX } from 'react'
import { IconCheckOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTask } from '../../../native.ts'
import { StatusChip } from '../../atoms/StatusChip.tsx'
import css from '../../blocks/TaskBoard.module.css'
import type { ChaosTranslate } from '../../locales.ts'
import { TASK_LANES, TASK_LANE_LABEL_KEY, TASK_TRANSITIONS, type TaskStatus } from './task-model.ts'

export function TaskStatusDropdown({ task, t, onMove }: {
  task: NativeTask
  t: ChaosTranslate
  onMove(target: TaskStatus): void
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

  const reachable = TASK_TRANSITIONS[task.status]
  return (
    <span ref={rootRef} className={css.statusDropdown}>
      <StatusChip status={task.status} label={t(TASK_LANE_LABEL_KEY[task.status])} title={t('tasks.statusChange')} onClick={() => { setOpen(current => !current) }} />
      {open && (
        <span role="menu" className={css.statusMenu}>
          {TASK_LANES.map(status => {
            const legal = reachable.includes(status)
            return (
              <button
                key={status}
                type="button"
                role="menuitem"
                className={css.statusMenuItem}
                disabled={!legal}
                title={legal ? undefined : t('tasks.statusIllegal')}
                onClick={() => { setOpen(false); onMove(status) }}
              >
                <span className={css.statusDot} data-status={status} aria-hidden="true" />
                <span className={css.statusMenuLabel}>{t(TASK_LANE_LABEL_KEY[status])}</span>
                {status === task.status && <IconCheckOutline16 />}
              </button>
            )
          })}
        </span>
      )}
    </span>
  )
}
