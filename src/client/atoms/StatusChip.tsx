/**
 * StatusChip — v2 (oil-creator StatusPill 模式): 原色点换宿主 `StateDot` 原语,
 * 仅 tint chip fill 保留(此为 raft 看板设计已批字型, 与宿主 Pill 无 vision 相等
 * variant 时不更换外殹)。已原地完成: 点 = StateDot, 内货余下由 atoms css 提。
 */
import { StateDot, type StateDotState } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTask } from '../../native.ts'
import css from './StatusChip.module.css'

/** task 四态 → StateDotState; in_review 紫无映射, 走本地色点。 */
const STATE_OF: Partial<Record<NativeTask['status'], StateDotState>> = {
  todo: 'ongoing',
  in_progress: 'warning',
  done: 'done',
}

export function StatusChip({ status, label, onClick, title, disabled, open }: {
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
