import type { JSX, ReactNode } from 'react'
import { classNames } from '../class-names.ts'
import css from './EntityRow.module.css'

export function EntityRow({ leading, title, description, meta, actions, selected, onSelect, className, ariaLabel }: {
  leading?: ReactNode
  title: ReactNode
  description?: ReactNode
  meta?: ReactNode
  actions?: ReactNode
  selected?: boolean | undefined
  onSelect?(): void
  className?: string | undefined
  ariaLabel?: string | undefined
}): JSX.Element {
  const copy = (
    <>
      {leading !== undefined && <span className={css.leading}>{leading}</span>}
      <span className={css.copy}>
        <span className={css.title}>{title}</span>
        {description !== undefined && <span className={css.description}>{description}</span>}
      </span>
      {meta !== undefined && <span className={css.meta}>{meta}</span>}
    </>
  )

  return (
    <div className={classNames(css.row, className)} data-selected={selected === true ? 'true' : undefined}>
      {onSelect === undefined
        ? <div className={css.content}>{copy}</div>
        : (
          <button
            type="button"
            className={css.content}
            aria-label={ariaLabel}
            aria-current={selected === true ? 'true' : undefined}
            onClick={onSelect}
          >
            {copy}
          </button>
        )}
      {actions !== undefined && <div className={css.actions}>{actions}</div>}
    </div>
  )
}
