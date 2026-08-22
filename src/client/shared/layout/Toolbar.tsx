import type { JSX, ReactNode } from 'react'
import { classNames } from '../class-names.ts'
import css from './Toolbar.module.css'

export function Toolbar({ start, end, className, label }: {
  start?: ReactNode
  end?: ReactNode
  className?: string | undefined
  label?: string | undefined
}): JSX.Element {
  return (
    <div className={classNames(css.toolbar, className)} role="toolbar" aria-label={label}>
      {start !== undefined && <div className={css.start}>{start}</div>}
      {end !== undefined && <div className={css.end}>{end}</div>}
    </div>
  )
}
