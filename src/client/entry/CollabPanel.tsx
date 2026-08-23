import { useEffect, useState, useSyncExternalStore, type JSX } from 'react'
import { Button, IconBrowseOutline16, IconChevronLeftOutline14, IconCloseOutline16, RiskConfirmation } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTarget } from '../../native.ts'
import type { CollabStore } from '../data/store.ts'
import { ChannelRail } from '../features/channels/ChannelRail.tsx'
import { ActivityView } from '../features/activity/ActivityView.tsx'
import { ChannelView } from '../features/channels/ChannelView.tsx'
import { ChannelCreateDialog } from '../features/channels/ChannelCreateDialog.tsx'
import { ChannelEditDialog } from '../features/channels/ChannelEditDialog.tsx'
import type { ChaosTranslate } from '../locales.ts'
import css from './CollabPanel.module.css'
import { CHAOS_NAVIGATE_CHANNEL_EVENT } from './navigation.ts'
import { EmptyState, ErrorBanner, IconButton, SkeletonList, StatusChip, Tabs } from '../shared/ui/index.ts'
import { ResponsiveDrilldown } from '../shared/layout/index.ts'

export interface CollabPanelProps {
  t: ChaosTranslate
  onClose: () => void
  store: CollabStore
  /** Active host locale id ('zh' | 'en'), read at render time for date labels. */
  activeLocale: () => string
}

/**
 * Collab overlay panel: header + channel rail + active channel view.
 */
export function CollabPanel({ t, onClose, store, activeLocale }: CollabPanelProps): JSX.Element {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot)
  const [createOpen, setCreateOpen] = useState(false)
  const [editChannel, setEditChannel] = useState<NativeTarget | undefined>(undefined)
  const [deleteChannel, setDeleteChannel] = useState<NativeTarget | undefined>(undefined)
  const [deleteAcknowledged, setDeleteAcknowledged] = useState(false)
  const [channelActionBusy, setChannelActionBusy] = useState(false)
  const [channelActionError, setChannelActionError] = useState<string | undefined>(undefined)
  /** 面板顶栏切视图(plocal Activity 为独立 shell section → 我们对应面板级)。 */
  const [panelView, setPanelView] = useState<'collab' | 'activity'>('collab')
  const [mobileChannelsOpen, setMobileChannelsOpen] = useState(false)
  /** Activity 行点击 → 新 ChannelView mount 初始值一次性消费 (key 换血保证 remount)。 */
  const [pendingThreadRoot, setPendingThreadRoot] = useState<string | undefined>(undefined)
  const active = state.channels.find(channel => channel.id === state.activeChannelId)

  const runLifecycle = (action: () => Promise<unknown>): void => {
    if (channelActionBusy) return
    setChannelActionBusy(true)
    setChannelActionError(undefined)
    void action().catch((reason: unknown) => {
      setChannelActionError(reason instanceof Error ? reason.message : String(reason))
    }).finally(() => { setChannelActionBusy(false) })
  }

  const confirmDelete = (): void => {
    const target = deleteChannel
    if (target === undefined || !deleteAcknowledged || channelActionBusy) return
    runLifecycle(async () => {
      await store.deleteChannel(target.id)
      setDeleteChannel(undefined)
      setDeleteAcknowledged(false)
    })
  }

  useEffect(() => {
    const showCollab = (): void => {
      setPanelView('collab')
      setMobileChannelsOpen(false)
      setPendingThreadRoot(undefined)
    }
    document.addEventListener(CHAOS_NAVIGATE_CHANNEL_EVENT, showCollab)
    return () => { document.removeEventListener(CHAOS_NAVIGATE_CHANNEL_EVENT, showCollab) }
  }, [])

  const channelRail = <ChannelRail
    t={t}
    state={state}
    onSelect={(targetId) => { setPendingThreadRoot(undefined); setMobileChannelsOpen(false); store.setActiveChannel(targetId) }}
    onCreate={() => { setCreateOpen(true) }}
    onEdit={(channel) => { setEditChannel(channel); setChannelActionError(undefined) }}
    onArchive={(channel) => { runLifecycle(() => store.archiveChannel(channel.id)) }}
    onRestore={(channel) => { runLifecycle(() => store.restoreChannel(channel.id)) }}
    onDelete={(channel) => { setDeleteChannel(channel); setDeleteAcknowledged(false); setChannelActionError(undefined) }}
  />
  const mainView = <main className={css.main}>
    {channelActionError !== undefined && <ErrorBanner>{t('channel.actionFailed', { error: channelActionError })}</ErrorBanner>}
    {state.removedNotice && (
      <div className={css.mainEmpty} role="status">
        <EmptyState title={t('channel.removed')} />
      </div>
    )}
    {!state.removedNotice && state.bootstrapError !== undefined && (
      <div className={css.mainEmpty} role="alert">
        <EmptyState
          title={t('panel.loadFailed', { error: state.bootstrapError })}
          action={<Button variant="outline" size="sm" onClick={() => { store.start() }}>{t('panel.retry')}</Button>}
        />
      </div>
    )}
    {!state.removedNotice && state.bootstrapError === undefined && !state.bootstrapped && (
      <div className={css.mainEmpty} role="status" aria-label={t('channel.loading')}>
        <SkeletonList className={css.mainSkeleton} rows={3} label={t('channel.loading')} />
      </div>
    )}
    {!state.removedNotice && state.bootstrapError === undefined && state.bootstrapped && active === undefined && (
      <EmptyState className={css.mainEmpty} icon={<IconBrowseOutline16 size={24} />} title={t('panel.selectChannel')} />
    )}
    {!state.removedNotice && active !== undefined && (
      <ChannelView
        key={active.id}
        t={t}
        store={store}
        state={state}
        channel={active}
        activeLocale={activeLocale}
        pendingThreadRoot={pendingThreadRoot}
        onPendingThreadConsumed={() => { setPendingThreadRoot(undefined) }}
        headerLeading={<span className={css.mobileChannelsButton}><IconButton label={t('panel.channels')} icon={<IconChevronLeftOutline14 size={14} />} onClick={() => { setMobileChannelsOpen(true) }} /></span>}
        headerActions={active.lifecycle === 'archived'
          ? <StatusChip tone="neutral" label={t('channel.archived')} />
          : undefined}
      />
    )}
  </main>
  const showMobileChannel = !mobileChannelsOpen && (
    active !== undefined || !state.bootstrapped || state.bootstrapError !== undefined || state.removedNotice
  )

  return (
    <>
      <header className={css.header}>
        <Tabs<'collab' | 'activity'>
          value={panelView}
          onValueChange={setPanelView}
          label={t('panel.title')}
          items={[
            { id: 'collab', label: t('panel.title') },
            { id: 'activity', label: t('activity.title') + ` (${String(state.activityCount)})` },
          ]}
        />
        <IconButton
          className={css.closeButton}
          label={t('panel.close')}
          icon={<IconCloseOutline16 size={16} />}
          onClick={onClose}
        />
      </header>
      {panelView === 'activity' ? (
        <div className={css.body}>
          <ActivityView
            t={t}
            store={store}
            state={state}
            activeLocale={activeLocale}
          />
        </div>
      ) : (
      <div className={css.body}>
        <ResponsiveDrilldown
          desktop={<>{channelRail}{mainView}</>}
          list={channelRail}
          detail={mainView}
          detailOpen={showMobileChannel}
        />
      </div>
      )}
      {createOpen && (
        <ChannelCreateDialog
          t={t}
          store={store}
          state={state}
          onClose={() => { setCreateOpen(false) }}
        />
      )}
      {editChannel !== undefined && (
        <ChannelEditDialog
          t={t}
          store={store}
          state={state}
          channel={editChannel}
          onClose={() => { setEditChannel(undefined) }}
        />
      )}
      {deleteChannel !== undefined && (
        <RiskConfirmation
          open
          title={t('channel.deleteTitle', { name: deleteChannel.name })}
          description={t('channel.deleteDescription')}
          acknowledgeLabel={t('channel.deleteAcknowledge')}
          cancelLabel={t('channel.deleteCancel')}
          confirmLabel={channelActionBusy ? t('channel.deleting') : t('channel.deleteConfirm')}
          acknowledged={deleteAcknowledged}
          disabled={channelActionBusy}
          onAcknowledgedChange={setDeleteAcknowledged}
          onCancel={() => { if (!channelActionBusy) setDeleteChannel(undefined) }}
          onConfirm={confirmDelete}
        />
      )}
    </>
  )
}
