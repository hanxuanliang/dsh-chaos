/**
 * StatusChip — task 状态 chip 唯一来源(点 + 文案, data-status 驱动色调).
 * 原子版: 由原 src/client/StatusChip.tsx 迁入 src/client/atoms/,
 * CSS 从 CollabPanel.module.css (.panel/.dialogBody scope) 迁到本模块。
 */
import type { JSX } from 'react'
import type { NativeTask } from '../../native.ts'
import css from './StatusChip.module.css'

type NativeTaskStatus = NativeTask['status']

export function StatusChip({ status, label, onClick, title, disabled, open }: {
  status: NativeTaskStatus
  label: string
  onClick?: () => void
  title?: string
  disabled?: boolean
  open?: boolean
}): JSX.Element {
  const dot = <span className={css.dot} data-status={status} aria-hidden="true" />
  if (onClick === undefined) {
    return (
      <span className={css.chip} data-status={status} title={title}>
        {dot}{label}
      </span>
    )
  }
  return (
    <button type="button" className={css.chip} data-status={status} data-open={open === true ? 'true' : undefined} title={title} disabled={disabled === true} onClick={onClick}>
      {dot}{label}
    </button>
  )
}
