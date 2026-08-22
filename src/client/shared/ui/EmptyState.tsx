import type { JSX, ReactNode } from 'react'
import { classNames } from '../class-names.ts'
import css from './EmptyState.module.css'

export function EmptyState({ title, description, icon, action, compact = false, className }: {
  title: ReactNode
  description?: ReactNode
  icon?: ReactNode
  action?: ReactNode
  compact?: boolean | undefined
  className?: string | undefined
}): JSX.Element {
  return (
    <div className={classNames(css.empty, className)} data-compact={compact ? 'true' : undefined}>
      {icon !== undefined && <div className={css.icon} aria-hidden="true">{icon}</div>}
      <strong>{title}</strong>
      {description !== undefined && <p>{description}</p>}
      {action !== undefined && <div className={css.action}>{action}</div>}
    </div>
  )
}
