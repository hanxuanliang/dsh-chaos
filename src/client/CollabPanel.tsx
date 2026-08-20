import { useState, useSyncExternalStore, type JSX } from 'react'
import { IconCloseOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { CollabStore } from './collab-store.ts'
import { ChannelRail } from './ChannelRail.tsx'
import { ActivityView } from './ActivityView.tsx'
import { ChannelView } from './ChannelView.tsx'
import { ChannelCreateDialog } from './ChannelCreateDialog.tsx'
import type { ChaosTranslate } from './locales.ts'
import css from './CollabPanel.module.css'
import { PillTabs } from './atoms/PillTabs.tsx'

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
  /** Activity 行点击 → 新 ChannelView mount 初始值一次性消费 (key 换血保证 remount)。 */
  const [pendingThreadRoot, setPendingThreadRoot] = useState<string | undefined>(undefined)
  const active = state.channels.find(channel => channel.id === state.activeChannelId)

  return (
    <>
      <header className={css.header}>
        <PillTabs
          items={[
            { id: 'collab', label: t('panel.title'), active: panelView === 'collab', onClick: () => { setPanelView('collab') } },
            { id: 'activity', label: t('activity.title') + ` (${String(state.activityCount)})`, active: panelView === 'activity', onClick: () => { setPanelView('activity') } },
          ]}
        />
        <button
          type="button"
          className={css.closeButton}
          aria-label={t('panel.close')}
          title={t('panel.close')}
          onClick={onClose}
        >
          <IconCloseOutline16 />
        </button>
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
        <ChannelRail
          t={t}
          state={state}
          onSelect={(targetId) => { setPendingThreadRoot(undefined); store.setActiveChannel(targetId) }}
          onCreate={() => { setCreateOpen(true) }}
        />
        <main className={css.main}>
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
              <svg viewBox="0 0 16 16" width="24" height="24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" aria-hidden="true">
                <path d="M9.5 2.5 14 8l-4.5 5.5M13.5 8H6M6 2.5 1.5 8 6 13.5" />
              </svg>
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
            />
          )}
        </main>
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
