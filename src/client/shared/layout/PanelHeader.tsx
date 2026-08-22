import type { JSX, ReactNode } from 'react'
import { IconChevronLeftOutline14 } from '@deepseek-ai/dsh-client-ui-primitives'
import { classNames } from '../class-names.ts'
import { IconButton } from '../ui/IconButton.tsx'
import css from './PanelHeader.module.css'

export function PanelHeader({ title, description, actions, backLabel, onBack, className }: {
  title: ReactNode
  description?: ReactNode
  actions?: ReactNode
  backLabel?: string | undefined
  onBack?(): void
  className?: string | undefined
}): JSX.Element {
  return (
    <header className={classNames(css.header, className)}>
      {onBack !== undefined && backLabel !== undefined && (
        <IconButton label={backLabel} icon={<IconChevronLeftOutline14 size={14} />} onClick={onBack} />
      )}
      <div className={css.copy}>
        <h2>{title}</h2>
        {description !== undefined && <p>{description}</p>}
      </div>
      {actions !== undefined && <div className={css.actions}>{actions}</div>}
    </header>
  )
}
