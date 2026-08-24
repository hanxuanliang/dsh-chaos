import { useId, useState, type JSX } from 'react'
import {
  IconChevronRightOutline14,
  IconEditOutline16,
  IconPlusOutline16,
  IconRefreshOutline16,
  IconTrashOutline16,
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
  onRestore(channel: NativeTarget): void
  onDelete(channel: NativeTarget): void
}

function ChannelRows({ channels, state, onSelect, onEdit, onRestore, onDelete, t }: {
  channels: NativeTarget[]
  state: CollabStoreSnapshot
  onSelect(targetId: string): void
  onEdit(channel: NativeTarget): void
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
    // 行内悬停图标替代下拉菜单(owner 审定: 菜单交互与面板风格不搭,
    // Linear rail 同款 hover-reveal) — 编辑进 dialog, 归档在 dialog 内
    // 危险区, 恢复/删除留在行内直达。活动行: 编辑+删除; 归档行: 恢复+删除。
    return (
      <div key={channel.id} role="listitem" className={css.railItem} data-active={active || undefined} data-unread={unread > 0 || undefined}>
        <button
          type="button"
          className={css.railRow}
          aria-current={active ? 'true' : undefined}
          onClick={() => { onSelect(channel.id) }}
        >
          {unread > 0 && <span className={css.railUnreadDot} aria-hidden="true" />}
          <span className={css.railHash} aria-hidden="true">#</span>
          <span className={css.railName}>{channel.name}</span>
          {unread > 0 && <span className={css.railUnread}>{unread}</span>}
        </button>
        {owner && (
          <span className={css.railActions}>
            {channel.lifecycle === 'archived'
              ? (
                <IconButton className={css.railAction} label={t('channel.restore')} icon={<IconRefreshOutline16 size={16} />}
                  onClick={() => { onRestore(channel) }} />
              )
              : (
                <IconButton className={css.railAction} label={t('channel.edit')} icon={<IconEditOutline16 size={16} />}
                  onClick={() => { onEdit(channel) }} />
              )}
            <IconButton className={css.railAction} label={t('channel.delete')} icon={<IconTrashOutline16 size={16} />} onClick={() => { onDelete(channel) }} />
          </span>
        )}
      </div>
    )
  })}</>
}

function GroupChevron({ open }: { open: boolean }): JSX.Element {
  return (
    <span className={css.railChevron} data-open={open ? 'true' : undefined} aria-hidden="true">
      <IconChevronRightOutline14 size={12} />
    </span>
  )
}

export function ChannelRail({ t, state, onSelect, onCreate, onEdit, onRestore, onDelete }: ChannelRailProps): JSX.Element {
  const [activeOpen, setActiveOpen] = useState(true)
  const [railFilter, setRailFilter] = useState<'all' | 'archived'>('all')
  const activeListId = useId()
  const activeChannels = state.channels.filter(channel => channel.lifecycle === 'active')
  const archivedChannels = state.channels.filter(channel => channel.lifecycle === 'archived')
  /* 恢复频道后归档变空: 筛选自动回 all, 避免停在空归档列表(实测 bug)。 */
  const showingArchived = railFilter === 'archived' && archivedChannels.length > 0
  const rows = { state, onSelect, onEdit, onRestore, onDelete, t }

  return (
    <nav className={css.rail} aria-label={t('panel.channels')}>
      <div className={css.railHead}>
        <button type="button" className={css.railTitle} aria-expanded={activeOpen} aria-controls={activeListId}
          data-channel-group-toggle="active" onClick={() => { setActiveOpen(open => !open) }}>
          <GroupChevron open={activeOpen} />
          <span className={css.railLabel}>{t('panel.channels')}</span>
          <span className={css.railCount}>{activeChannels.length}</span>
        </button>
        <button type="button" className={css.railAdd} aria-label={t('panel.railCreate')} title={t('panel.railCreate')} onClick={onCreate}>
          <IconPlusOutline16 size={14} />
        </button>
      </div>
      <div className={css.railFilterRow} role="group" aria-label={t('panel.railFilterAria')}>
        <button type="button" className={css.railFilter} data-active={!showingArchived || undefined}
          aria-pressed={!showingArchived} onClick={() => { setRailFilter('all') }}>{t('activity.filterAll')}</button>
        <button type="button" className={css.railFilter} data-active={showingArchived || undefined}
          aria-pressed={showingArchived} disabled={archivedChannels.length === 0}
          onClick={() => { setRailFilter('archived') }}>{t('channel.archivedGroup')}</button>
      </div>
      <div className={css.railScroll}>
        {showingArchived ? (
          archivedChannels.length > 0 && (
            <div data-channel-group-list="archived" role="list">
              <ChannelRows channels={archivedChannels} {...rows} />
            </div>
          )
        ) : activeOpen && (
          <div id={activeListId} data-channel-group-list="active" role="list">
            {activeChannels.length === 0 && <p className={css.railEmpty}>{t('panel.railEmpty')}</p>}
            <ChannelRows channels={activeChannels} {...rows} />
          </div>
        )}
      </div>
    </nav>
  )
}
