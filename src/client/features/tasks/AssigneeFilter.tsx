import { useState, type JSX } from 'react'
import { IconChevronDownOutline14, IconUserOutline16, Menu } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor } from '../../../native.ts'
import type { ChaosTranslate } from '../../locales.ts'
import css from '../../blocks/TaskBoard.module.css'

export function AssigneeFilter({ t, members, value, onChange }: {
  t: ChaosTranslate
  members: NativeActor[]
  value: string
  onChange(value: string): void
}): JSX.Element {
  const [open, setOpen] = useState(false)
  const label = value === ''
    ? t('tasks.filterAssignee')
    : value === 'unassigned'
      ? t('tasks.unassigned')
      : `@${members.find(member => member.id === value)?.handle ?? '?'}`

  return (
    <Menu
      open={open}
      portal
      compact
      dense
      onClose={() => { setOpen(false) }}
      onSelect={(id) => { onChange(id === 'all' ? '' : id); setOpen(false) }}
      selectedId={value === '' ? 'all' : value}
      items={[
        { id: 'all', label: t('tasks.filterAll') },
        { id: 'unassigned', label: t('tasks.unassigned') },
        { type: 'separator', id: 'sep' },
        ...members.map(member => ({ id: member.id, label: `@${member.handle}` })),
      ]}
      anchor={(
        <button
          type="button"
          className={`${css.filterPill} ${value !== '' ? css.filterPillActive : ''}`}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => { setOpen(current => !current) }}
        >
          <IconUserOutline16 aria-hidden="true" />
          {label}
          <IconChevronDownOutline14 />
        </button>
      )}
    />
  )
}
