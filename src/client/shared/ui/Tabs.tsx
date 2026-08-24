import { useRef, type JSX, type KeyboardEvent, type ReactNode } from 'react'
import { classNames } from '../class-names.ts'
import css from './Tabs.module.css'

export interface TabItem<T extends string> {
  id: T
  label: ReactNode
  disabled?: boolean | undefined
  title?: string | undefined
  tabId?: string | undefined
  panelId?: string | undefined
}

export function Tabs<T extends string>({ items, value, onValueChange, label, align = 'inline', variant, className }: {
  items: Array<TabItem<T>>
  value: T
  onValueChange(value: T): void
  label: string
  align?: 'inline' | 'lead' | 'stretch' | undefined
  /** 'underline' swaps the segmented-pill grammar for the synapse canvas-tabs
   * grammar (bare labels + primary underline on the container's bottom edge).
   * 'floating' renders the synapse view-switch grammar — an absolutely
   * centered full-round pill group hovering over the header's contents.
   * Default keeps the existing segmented pill. */
  variant?: 'underline' | 'floating' | undefined
  className?: string | undefined
}): JSX.Element {
  const rootRef = useRef<HTMLDivElement>(null)
  const selectFromKey = (event: KeyboardEvent<HTMLButtonElement>): void => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
    const buttons = [...rootRef.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]:not(:disabled)') ?? []]
    if (buttons.length === 0) return
    event.preventDefault()
    const current = buttons.indexOf(event.currentTarget)
    const next = event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? buttons.length - 1
        : (current + (event.key === 'ArrowRight' ? 1 : -1) + buttons.length) % buttons.length
    buttons[next]?.focus()
    buttons[next]?.click()
  }

  return (
    <div ref={rootRef} className={classNames(css.tabs, className)} data-align={align} data-variant={variant} role="tablist" aria-label={label}>
      {items.map((item) => {
        const active = item.id === value
        return (
          <button
            key={item.id}
            id={item.tabId}
            type="button"
            role="tab"
            className={css.tab}
            aria-selected={active}
            aria-controls={item.panelId}
            tabIndex={active ? 0 : -1}
            disabled={item.disabled}
            title={item.title}
            data-active={active ? 'true' : undefined}
            onClick={() => { onValueChange(item.id) }}
            onKeyDown={selectFromKey}
          >
            {item.label}
          </button>
        )
      })}
    </div>
  )
}
