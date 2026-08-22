import { useEffect, useState, useSyncExternalStore, type JSX } from 'react'
import { IconBrowseOutline16, IconChevronLeftOutline14, IconCloseOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { CollabStore } from '../data/store.ts'
import { ChannelRail } from '../features/channels/ChannelRail.tsx'
import { ActivityView } from '../features/activity/ActivityView.tsx'
import { ChannelView } from '../features/channels/ChannelView.tsx'
import { ChannelCreateDialog } from '../features/channels/ChannelCreateDialog.tsx'
import type { ChaosTranslate } from '../locales.ts'
import css from './CollabPanel.module.css'
import { CHAOS_NAVIGATE_CHANNEL_EVENT } from './navigation.ts'
import { IconButton, Tabs } from '../shared/ui/index.ts'
import { ResponsiveDrilldown } from '../shared/layout/index.ts'

export interface CollabPanelProps {
  t: ChaosTranslate
  onClose: () => void
  store: CollabStore
  /** Active host locale id ('zh' | 'en'), read at render time for date labels. */
  activeLocale: () => string
}

/**
 * Collab overlay panel (P0-2): header + channel rail + active channel view.
 * P0-1 landed the skeleton and full-pane takeover; this revision fills the
 * rail and main region with live chaos-kernel data (channels, message
 * history, composer). Thread surface stays a later milestone (P0-5).
 */
export function CollabPanel({ t, onClose, store, activeLocale }: CollabPanelProps): JSX.Element {
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot)
  const [createOpen, setCreateOpen] = useState(false)
  /** 面板顶栏切视图(plocal Activity 为独立 shell section → 我们对应面板级)。 */
  const [panelView, setPanelView] = useState<'collab' | 'activity'>('collab')
  const [mobileChannelsOpen, setMobileChannelsOpen] = useState(false)
  /** Activity 行点击 → 新 ChannelView mount 初始值一次性消费 (key 换血保证 remount)。 */
  const [pendingThreadRoot, setPendingThreadRoot] = useState<string | undefined>(undefined)
  const active = state.channels.find(channel => channel.id === state.activeChannelId)

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
  />
  const mainView = <main className={css.main}>
    {state.removedNotice && (
      <div className={css.mainEmpty} role="status">
        <p className={css.empty}>{t('channel.removed')}</p>
      </div>
    )}
    {!state.removedNotice && state.bootstrapError !== undefined && (
      <div className={css.mainEmpty} role="alert">
        <p className={css.empty}>{t('panel.loadFailed', { error: state.bootstrapError })}</p>
        <button type="button" className={css.retryButton} onClick={() => { store.start() }}>
          {t('panel.retry')}
        </button>
      </div>
    )}
    {!state.removedNotice && state.bootstrapError === undefined && !state.bootstrapped && (
      <div className={css.mainEmpty} role="status" aria-label={t('channel.loading')}>
        <div className={css.skeletonRow} />
        <div className={css.skeletonRow} />
        <div className={css.skeletonRow} />
      </div>
    )}
    {!state.removedNotice && state.bootstrapError === undefined && state.bootstrapped && active === undefined && (
      <div className={css.mainEmpty}>
        <IconBrowseOutline16 size={24} />
        <p className={css.empty}>{t('panel.selectChannel')}</p>
      </div>
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
    </>
  )
}
