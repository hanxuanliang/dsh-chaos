/**
 * Secondary rail of the collab panel: channel navigation (spec §1.1).
 * Group header anatomy borrows plocal's ChatSidebar section header (uppercase
 * wide-tracked label + functional chevron collapse + count + always-visible
 * "+"); the "+" is the only channel-create entry. Unread = tail.count minus
 * the localStorage read marker (no backend read markers exist).
 */
import { useState, type JSX } from 'react'
import { IconPlusOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { CollabStoreSnapshot } from './collab-store.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './blocks/ChannelRail.module.css'

export interface ChannelRailProps {
  t: ChaosTranslate
  state: CollabStoreSnapshot
  onSelect(targetId: string): void
  onCreate(): void
}

export function ChannelRail({ t, state, onSelect, onCreate }: ChannelRailProps): JSX.Element {
  const [open, setOpen] = useState(true)
  return (
    <nav className={css.rail} aria-label={t('panel.channels')}>
      <div className={css.railHead}>
        <button
          type="button"
          className={css.railTitle}
          aria-expanded={open}
          onClick={() => { setOpen(!open) }}
        >
          <svg viewBox="0 0 16 16" width="10" height="10" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d={open ? 'M4 6l4 4 4-4' : 'M6 4l4 4-4 4'} />
          </svg>
          <span className={css.railLabel}>{t('panel.channels')}</span>
          <span className={css.railCount}>{state.channels.length}</span>
        </button>
        <button
          type="button"
          className={css.railAdd}
          aria-label={t('panel.railCreate')}
          title={t('panel.railCreate')}
          onClick={onCreate}
        >
          <IconPlusOutline16 size={14} />
        </button>
      </div>
      {open && <div className={css.railScroll} role="list">
        {state.channels.length === 0 && <p className={css.railEmpty}>{t('panel.railEmpty')}</p>}
        {state.channels.map((channel) => {
          const active = channel.id === state.activeChannelId
          const unread = state.unreadByChannel[channel.id] ?? 0
          return (
            <button
              key={channel.id}
              type="button"
              role="listitem"
              className={css.railRow}
              data-active={active || undefined}
              aria-current={active ? 'true' : undefined}
              onClick={() => { onSelect(channel.id) }}
            >
              <span className={css.railHash} aria-hidden="true">#</span>
              <span className={css.railName}>{channel.name}</span>
              {unread > 0 && <span className={css.railUnread}>{unread}</span>}
            </button>
          )
        })}
      </div>}
    </nav>
  )
}
