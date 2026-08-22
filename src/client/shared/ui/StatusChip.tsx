import type { JSX, ReactNode } from 'react'
import { StateDot, type StateDotState } from '@deepseek-ai/dsh-client-ui-primitives'
import { classNames } from '../class-names.ts'
import css from './StatusChip.module.css'

export type StatusTone = 'neutral' | 'info' | 'success' | 'warning' | 'error'

const STATE_BY_TONE: Record<Exclude<StatusTone, 'neutral' | 'error'>, StateDotState> = {
  info: 'ongoing',
  success: 'done',
  warning: 'warning',
}

export function StatusChip({ label, tone = 'neutral', className }: {
  label: ReactNode
  tone?: StatusTone | undefined
  className?: string | undefined
}): JSX.Element {
  const state = tone === 'neutral' || tone === 'error' ? undefined : STATE_BY_TONE[tone]
  return (
    <span className={classNames(css.chip, className)} data-tone={tone}>
      {state === undefined
        ? <span className={css.dot} aria-hidden="true" />
        : <StateDot state={state} size={8} />}
      <span>{label}</span>
    </span>
  )
}
