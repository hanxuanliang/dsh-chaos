/**
 * StatusChip — task 状态 chip 唯一来源(点 + 文案,唯一调色板 token 在 CSS);
 * 复用面: 看板头部状态下拉 toggle / Activity 行里任务锚 chip。
 * 需要交互版由调用方包按鈆并传 onClick; 只读版直接 messages。
 */
import type { JSX } from 'react'
import css from './CollabPanel.module.css'
import type { NativeTask } from '../native.ts'

type NativeTaskStatus = NativeTask['status']

export function StatusChip({ status, label, onClick, title, disabled, open }: {
  status: NativeTaskStatus
  label: string
  onClick?: () => void
  title?: string
  disabled?: boolean
  open?: boolean
}): JSX.Element {
  const dot = <span className={css.statusDot} data-status={status} aria-hidden="true" />
  if (onClick === undefined) {
    return (
      <span className={css.statusChip} data-status={status} title={title}>
        {dot}{label}
      </span>
    )
  }
  return (
    <button type="button" className={css.statusChip} data-status={status} data-open={open === true ? 'true' : undefined} title={title} disabled={disabled === true} onClick={onClick}>
      {dot}{label}
    </button>
  )
}
