import type { JSX, ReactNode } from 'react'
import { classNames } from '../class-names.ts'
import css from './ErrorBanner.module.css'

export function ErrorBanner({ children, action, className }: {
  children: ReactNode
  action?: ReactNode
  className?: string | undefined
}): JSX.Element {
  return (
    <div className={classNames(css.banner, className)} role="alert">
      <span className={css.message}>{children}</span>
      {action !== undefined && <span className={css.action}>{action}</span>}
    </div>
  )
}
