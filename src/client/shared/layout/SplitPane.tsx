import type { JSX, ReactNode } from 'react'
import { Group, Panel, Separator, useDefaultLayout } from 'react-resizable-panels'
import { classNames } from '../class-names.ts'
import css from './SplitPane.module.css'

const safeStorage = {
  getItem(key: string): string | null {
    try { return window.localStorage.getItem(key) } catch { return null }
  },
  setItem(key: string, value: string): void {
    try { window.localStorage.setItem(key, value) } catch { /* Persistence is optional. */ }
  },
}

export function SplitPane({ id, leading, trailing, leadingDefault = 320, leadingMin = 280, leadingMax = 480, trailingMin = 420, separatorLabel, className }: {
  id: string
  leading: ReactNode
  trailing: ReactNode
  leadingDefault?: number | undefined
  leadingMin?: number | undefined
  leadingMax?: number | undefined
  trailingMin?: number | undefined
  separatorLabel: string
  className?: string | undefined
}): JSX.Element {
  const leadingId = `${id}-leading`
  const trailingId = `${id}-trailing`
  const { defaultLayout, onLayoutChanged } = useDefaultLayout({
    id: `dsh-chaos:${id}`,
    panelIds: [leadingId, trailingId],
    storage: safeStorage,
    onlySaveAfterUserInteractions: true,
  })

  return (
    <Group
      id={id}
      className={classNames(css.group, className)}
      orientation="horizontal"
      defaultLayout={defaultLayout}
      onLayoutChanged={onLayoutChanged}
      resizeTargetMinimumSize={{ fine: 8, coarse: 24 }}
    >
      <Panel
        id={leadingId}
        className={css.panel}
        defaultSize={leadingDefault}
        minSize={leadingMin}
        maxSize={leadingMax}
        groupResizeBehavior="preserve-pixel-size"
      >
        {leading}
      </Panel>
      <Separator id={`${id}-separator`} className={css.separator} aria-label={separatorLabel}>
        <span className={css.rule} />
      </Separator>
      <Panel id={trailingId} className={css.panel} minSize={trailingMin}>
        {trailing}
      </Panel>
    </Group>
  )
}
