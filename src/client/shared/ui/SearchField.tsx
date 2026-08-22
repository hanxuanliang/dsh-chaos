import { type ChangeEvent, type InputHTMLAttributes, type JSX } from 'react'
import { IconCloseOutline16, IconSearchOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import { classNames } from '../class-names.ts'
import { IconButton } from './IconButton.tsx'
import css from './SearchField.module.css'

export function SearchField({ value, onValueChange, label, clearLabel, className, ...props }: {
  value: string
  onValueChange(value: string): void
  label: string
  clearLabel: string
  className?: string | undefined
} & Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'value' | 'onChange' | 'aria-label'>): JSX.Element {
  const change = (event: ChangeEvent<HTMLInputElement>): void => { onValueChange(event.target.value) }
  return (
    <label className={classNames(css.search, className)}>
      <span className={css.srOnly}>{label}</span>
      <IconSearchOutline16 size={16} className={css.searchIcon} />
      <input {...props} type="search" value={value} onChange={change} aria-label={label} />
      {value !== '' && (
        <IconButton
          className={css.clear}
          label={clearLabel}
          icon={<IconCloseOutline16 size={14} />}
          tooltip={false}
          onClick={() => { onValueChange('') }}
        />
      )}
    </label>
  )
}
