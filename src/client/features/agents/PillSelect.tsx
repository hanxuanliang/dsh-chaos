import { useState, type JSX } from 'react'
import { IconChevronDownOutline14, Menu, type MenuEntry } from '@deepseek-ai/dsh-client-ui-primitives'
import css from './PillSelect.module.css'

interface PillSelectProps {
  label: string
  placeholder: string
  value: string
  entries: readonly MenuEntry[]
  selectedId: string
  disabled: boolean
  onSelect(id: string): void
}

/**
 * Circle-style metadata pill: a small trigger button showing the current
 * value, opening the DSH Menu primitive as the option list. The pill row
 * replaces the raw `<select>` trio so every control in the creation form
 * speaks the same DSH visual grammar.
 */
export function PillSelect(props: PillSelectProps): JSX.Element {
  const { label, placeholder, value, entries, selectedId, disabled, onSelect } = props
  const [open, setOpen] = useState(false)
  const filled = value !== ''
  return (
    <Menu
      open={open}
      onClose={() => { setOpen(false) }}
      items={entries}
      selectedId={selectedId === '' ? undefined : selectedId}
      onSelect={id => { onSelect(id); setOpen(false) }}
      compact
      anchor={(
        <button
          type="button"
          className={css.pill}
          aria-haspopup="listbox"
          aria-expanded={open}
          aria-label={label}
          title={filled ? value : placeholder}
          disabled={disabled}
          data-filled={filled ? 'true' : undefined}
          onClick={() => { setOpen(state => !state) }}
          data-open={open ? 'true' : undefined}
        >
          {!filled && <span className={css.pillLabel}>{label}</span>}
          <span className={filled ? css.pillValue : css.pillPlaceholder}>
            {filled ? value : placeholder}
          </span>
          <IconChevronDownOutline14 size={12} className={css.pillChevron} aria-hidden="true" />
        </button>
      )}
    />
  )
}
