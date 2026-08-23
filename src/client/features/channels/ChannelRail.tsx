import { useState, type JSX } from 'react'
import {
  IconArchiveOutline20,
  IconChevronRightOutline14,
  IconEditOutline16,
  IconEllipsisOutline16,
  IconPlusOutline16,
  IconRefreshOutline16,
  IconTrashOutline16,
  Menu,
  type MenuEntry,
} from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTarget } from '../../../native.ts'
import type { CollabStoreSnapshot } from '../../data/store.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { IconButton } from '../../shared/ui/index.ts'
import css from './ChannelRail.module.css'

export interface ChannelRailProps {
  t: ChaosTranslate
  state: CollabStoreSnapshot
  onSelect(targetId: string): void
  onCreate(): void
  onEdit(channel: NativeTarget): void
  onArchive(channel: NativeTarget): void
  onRestore(channel: NativeTarget): void
  onDelete(channel: NativeTarget): void
}

function ChannelRows({ channels, state, menuId, setMenuId, onSelect, onEdit, onArchive, onRestore, onDelete, t }: {
  channels: NativeTarget[]
  state: CollabStoreSnapshot
  menuId: string | undefined
  setMenuId(id: string | undefined): void
  onSelect(targetId: string): void
  onEdit(channel: NativeTarget): void
  onArchive(channel: NativeTarget): void
  onRestore(channel: NativeTarget): void
  onDelete(channel: NativeTarget): void
  t: ChaosTranslate
}): JSX.Element {
  return <>{channels.map((channel) => {
    const active = channel.id === state.activeChannelId
    const unread = state.unreadByChannel[channel.id] ?? 0
    const memberships = state.membershipsByChannel[channel.id]
    const owner = memberships === undefined
      ? channel.createdBy === state.selfId
      : memberships.some(membership => membership.actor.id === state.selfId && membership.role === 'owner')
    const items: MenuEntry[] = channel.lifecycle === 'archived'
      ? [
          { id: 'restore', label: t('channel.restore'), icon: <IconRefreshOutline16 size={16} /> },
          { type: 'separator', id: 'danger' },
          { id: 'delete', label: t('channel.delete'), icon: <IconTrashOutline16 size={16} />, danger: true },
        ]
      : [
          { id: 'edit', label: t('channel.edit'), icon: <IconEditOutline16 size={16} /> },
          { id: 'archive', label: t('channel.archive'), icon: <IconArchiveOutline20 size={16} /> },
          { type: 'separator', id: 'danger' },
          { id: 'delete', label: t('channel.delete'), icon: <IconTrashOutline16 size={16} />, danger: true },
        ]
    return (
      <div key={channel.id} role="listitem" className={css.railItem} data-active={active || undefined}>
        <button
          type="button"
          className={css.railRow}
          aria-current={active ? 'true' : undefined}
          onClick={() => { onSelect(channel.id) }}
        >
          <span className={css.railHash} aria-hidden="true">#</span>
          <span className={css.railName}>{channel.name}</span>
          {unread > 0 && <span className={css.railUnread}>{unread}</span>}
        </button>
        {owner && (
          <Menu
            open={menuId === channel.id}
            anchor={(
              <IconButton
                className={css.railMore}
                label={t('channel.actions', { name: channel.name })}
                icon={<IconEllipsisOutline16 size={16} />}
                tooltip={false}
                aria-haspopup="menu"
                aria-expanded={menuId === channel.id}
                onClick={() => { setMenuId(menuId === channel.id ? undefined : channel.id) }}
              />
            )}
            items={items}
            portal
            align="end"
            compact
            onClose={() => { setMenuId(undefined) }}
            onSelect={(id) => {
              setMenuId(undefined)
              if (id === 'edit') onEdit(channel)
              else if (id === 'archive') onArchive(channel)
              else if (id === 'restore') onRestore(channel)
              else if (id === 'delete') onDelete(channel)
            }}
          />
        )}
      </div>
    )
  })}</>
}

export function ChannelRail({ t, state, onSelect, onCreate, onEdit, onArchive, onRestore, onDelete }: ChannelRailProps): JSX.Element {
  const [activeOpen, setActiveOpen] = useState(true)
  const [archivedOpen, setArchivedOpen] = useState(false)
  const [menuId, setMenuId] = useState<string | undefined>(undefined)
  const activeChannels = state.channels.filter(channel => channel.lifecycle === 'active')
  const archivedChannels = state.channels.filter(channel => channel.lifecycle === 'archived')
  const rows = { state, menuId, setMenuId, onSelect, onEdit, onArchive, onRestore, onDelete, t }

  return (
    <nav className={css.rail} aria-label={t('panel.channels')}>
      <div className={css.railHead}>
        <button type="button" className={css.railTitle} aria-expanded={activeOpen} onClick={() => { setActiveOpen(!activeOpen) }}>
          <IconChevronRightOutline14 className={css.railChevron} data-open={activeOpen ? 'true' : undefined} size={10} />
          <span className={css.railLabel}>{t('panel.channels')}</span>
          <span className={css.railCount}>{activeChannels.length}</span>
        </button>
        <button type="button" className={css.railAdd} aria-label={t('panel.railCreate')} title={t('panel.railCreate')} onClick={onCreate}>
          <IconPlusOutline16 size={14} />
        </button>
      </div>
      <div className={css.railScroll}>
        {activeOpen && (
          <div role="list">
            {activeChannels.length === 0 && <p className={css.railEmpty}>{t('panel.railEmpty')}</p>}
            <ChannelRows channels={activeChannels} {...rows} />
          </div>
        )}
        {archivedChannels.length > 0 && (
          <section className={css.archiveGroup}>
            <button type="button" className={css.railTitle} aria-expanded={archivedOpen} onClick={() => { setArchivedOpen(!archivedOpen) }}>
              <IconChevronRightOutline14 className={css.railChevron} data-open={archivedOpen ? 'true' : undefined} size={10} />
              <span className={css.railLabel}>{t('channel.archivedGroup')}</span>
              <span className={css.railCount}>{archivedChannels.length}</span>
            </button>
            {archivedOpen && <div role="list"><ChannelRows channels={archivedChannels} {...rows} /></div>}
          </section>
        )}
      </div>
    </nav>
  )
}
