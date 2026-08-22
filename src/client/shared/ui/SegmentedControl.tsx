import type { JSX, ReactNode } from 'react'
import { classNames } from '../class-names.ts'
import css from './SegmentedControl.module.css'

export interface SegmentItem<T extends string> {
  id: T
  label: ReactNode
  disabled?: boolean | undefined
}

export function SegmentedControl<T extends string>({ items, value, onValueChange, label, className }: {
  items: Array<SegmentItem<T>>
  value: T
  onValueChange(value: T): void
  label: string
  className?: string | undefined
}): JSX.Element {
  return (
    <div className={classNames(css.group, className)} role="group" aria-label={label}>
      {items.map((item) => {
        const active = item.id === value
        return (
          <button
            key={item.id}
            type="button"
            className={css.segment}
            aria-pressed={active}
            disabled={item.disabled}
            data-active={active ? 'true' : undefined}
            onClick={() => { onValueChange(item.id) }}
          >
            {item.label}
          </button>
        )
      })}
    </div>
  )
}
