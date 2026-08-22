import type { JSX } from 'react'
import { classNames } from '../class-names.ts'
import css from './SkeletonList.module.css'

export function SkeletonList({ rows = 5, className, label = 'Loading' }: {
  rows?: number | undefined
  className?: string | undefined
  label?: string | undefined
}): JSX.Element {
  return (
    <div className={classNames(css.list, className)} role="status" aria-label={label}>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className={css.row} aria-hidden="true">
          <span className={css.leading} />
          <span className={css.copy}><span /><span /></span>
          <span className={css.meta} />
        </div>
      ))}
    </div>
  )
}
