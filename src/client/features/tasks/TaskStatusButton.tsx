/** Interactive task-state control using the host StateDot and task-owned tint. */
import { StateDot, type StateDotState } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTask } from '../../../native.ts'
import css from './TaskStatusButton.module.css'

/** Task states mapped to host dots; review keeps its domain-specific color. */
const STATE_OF: Partial<Record<NativeTask['status'], StateDotState>> = {
  todo: 'ongoing',
  in_progress: 'warning',
  done: 'done',
}

export function TaskStatusButton({ status, label, onClick, title, disabled, open }: {
  status: NativeTask['status']
  label: string
  onClick?: (() => void) | undefined
  title?: string | undefined
  disabled?: boolean | undefined
  open?: boolean | undefined
}) {
  const state = STATE_OF[status]
  return (
    <button
      type="button"
      className={css.chip}
      data-status={status}
      onClick={onClick}
      disabled={disabled}
      title={title ?? label}
      {...(open !== undefined ? { 'data-open': open ? 'true' : 'false' } : {})}
    >
      {state !== undefined
        ? <StateDot state={state} size={10} />
        : <span className={css.dot} data-status={status} aria-hidden="true" />}
      {label}
    </button>
  )
}
