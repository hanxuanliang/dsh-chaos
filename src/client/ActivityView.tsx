/**
 * Activity 视图(rc-4 统一性整改):
 * - 行卡: 目标小行 → 主标题(粗) → 最新回复 → 底行(task chip+N replies);
 *   不再有前置 kind 图标(点击自然开右栏)。
 * - 点击 = 列表收窄 + 右栏 dock,channel/thread 同一套 dock 机制, 不再整页跳:
 *   channel → DockedChannelPane(header-lite + MessageStream + ChannelComposer);
 *   thread → ThreadPanel 本体(与频道内同一件)。
 * - 组件复用: filter 使用 pressed-button SegmentedControl，详情用共享 SplitPane；
 *   composer = ChannelComposer，消息流 = MessageStream。
 * - Unread / All 与 Mark-all-read 都接真实 read vertical，不做假交互。
 * - direct 行暂不做(DM 主界面没建,点击没有诚实目标 — 隐藏)。
 */
import { useMemo, useRef, useState, type JSX } from 'react'
import { Button, IconCheckOutline16, IconCloseOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActivityInboxItem, NativeTarget } from '../native.ts'
import css from './blocks/ActivityView.module.css'
import type { ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import { ChannelView } from './ChannelView.tsx'
import { ThreadPanel } from './ThreadPanel.tsx'
import { ActivityCard } from './blocks/ActivityCard.tsx'
import cardCss from './blocks/ActivityCard.module.css'
import { EmptyState, ErrorBanner, IconButton, SegmentedControl } from './shared/ui/index.ts'
import { PanelHeader, ResponsiveDrilldown, SplitPane, Toolbar } from './shared/layout/index.ts'
interface ActivityViewProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  activeLocale(): string
}
/**
 * dock 状态: 右栏 **就是那套 channel/thread 内容区**——channel-first,
 * 有 threadRootId 时才在旁边展开 (ChannelChatPane 同一件)。
 */
interface Dock { channelId: string, threadRootId?: string }
function relativeTime(atMs: number): string {
  const deltaSeconds = Math.max(0, Math.floor((Date.now() - atMs) / 1000))
  if (deltaSeconds < 60) return '1m'
  const minutes = Math.floor(deltaSeconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  return new Date(atMs).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}
export function ActivityView({ t, store, state, activeLocale }: ActivityViewProps): JSX.Element {
  const [busy, setBusy] = useState<string | undefined>(undefined)
  const [error, setError] = useState<string | undefined>(undefined)
  const lastTriggerId = useRef<string | null>(null)
  const actorNamesById = new Map<string, string>()
  for (const a of state.actors) actorNamesById.set(a.id, a.displayName)

  const [dock, setDock] = useState<Dock | undefined>(undefined)
  const items = useMemo(
    () => state.activityItems.filter((item) => item.targetKind !== 'direct'),
    [state.activityItems],
  )
  const dockThread = dock?.threadRootId !== undefined && dock !== undefined
    ? state.threads.find((x) => x.rootMessageId === dock.threadRootId)
    : undefined
  const dockKey = dock === undefined ? undefined : `${dock.channelId}:${dock.threadRootId ?? ''}`
  const closeDock = (): void => {
    setDock(undefined)
    window.requestAnimationFrame(() => {
      const trigger = [...document.querySelectorAll<HTMLButtonElement>('[data-activity-id]')]
        .find(button => button.dataset.activityId === lastTriggerId.current)
      trigger?.focus()
    })
  }
  const open = (item: NativeActivityInboxItem, trigger?: HTMLButtonElement): void => {
    if (trigger !== undefined) lastTriggerId.current = item.conversationId
    if (item.targetKind === 'channel') {
      void store.hydrateTarget(item.conversationId)
      setDock((cur) =>
        cur !== undefined && cur.channelId === item.conversationId && cur.threadRootId === undefined
          ? undefined
          : { channelId: item.conversationId },
      )
      return
    }
    if (item.targetKind === 'thread' && item.rootMessageId !== undefined && item.parentTargetId !== undefined) {
      const rootMessageId = item.rootMessageId
      const parentChannelId = item.parentTargetId
      void store.openThread(rootMessageId)
      void store.hydrateTarget(parentChannelId) // root 卡取自父频道历史
      setDock((cur) =>
        cur !== undefined && cur.threadRootId === rootMessageId
          ? undefined
          : { channelId: parentChannelId, threadRootId: rootMessageId },
      )
    }
  }
  const markAllDone = (): void => {
    setBusy('all')
    setError(undefined)
    store.markAllActivityDone()
      .catch((e: unknown) => {
        setError(t('activity.markAllFailed', { error: e instanceof Error ? e.message : String(e) }))
      })
      .finally(() => { setBusy(undefined) })
  }
  const markDone = (item: NativeActivityInboxItem): void => {
    setBusy(item.conversationId)
    setError(undefined)
    store.markActivityDone(item.conversationId, item.lastActivitySeq)
      .catch((e: unknown) => {
        setError(t('activity.doneFailed', { error: e instanceof Error ? e.message : String(e) }))
      })
      .finally(() => { setBusy(undefined) })
  }
  const isDockSelected = (item: NativeActivityInboxItem): boolean => {
    if (dock === undefined) return false
    if (item.targetKind === 'channel') return dock.channelId === item.conversationId && dock.threadRootId === undefined
    return item.rootMessageId !== undefined && dock.threadRootId === item.rootMessageId
  }
  const filter = state.activityFilter
  const unreadCount = state.activityCount
  const filters = <SegmentedControl
    label={t('activity.filtersAria')}
    value={filter}
    onValueChange={value => { void store.setActivityFilter(value) }}
    items={[
      { id: 'unread', label: unreadCount > 0 ? `${t('activity.filterUnread')} (${unreadCount})` : t('activity.filterUnread') },
      { id: 'all', label: t('activity.filterAll') },
    ]}
  />
  const markAll = unreadCount > 0 ? (
    <Button
      className={css.markAllButton}
      variant="ghost"
      size="sm"
      icon={<IconCheckOutline16 size={14} />}
      disabled={busy !== undefined}
      onClick={() => { markAllDone() }}
    >
      {t('activity.markAllRead')}
    </Button>
  ) : undefined
  const rows = (
    <div className={cardCss.list} role="list">
      {items.map((item) => (
        <ActivityCard
          key={item.conversationId}
          item={item}
          t={t}
          timeLabel={relativeTime(item.lastActivityAtMs)}
          selected={isDockSelected(item)}
          busy={busy === item.conversationId}
          thread={item.rootMessageId === undefined ? undefined : state.threads.find(th => th.rootMessageId === item.rootMessageId)}
          summary={item.rootMessageId === undefined ? undefined : state.threadSummariesByRoot[item.rootMessageId]}
          actorNamesById={actorNamesById}
          onOpen={trigger => { open(item, trigger) }}
          onDone={() => { markDone(item) }}
        />
      ))}
    </div>
 )
  const listPane = (
    <section className={css.activityListCol} aria-label={t('activity.title')}>
      <PanelHeader title={t('activity.title')} />
      <Toolbar start={filters} end={markAll} />
      {error !== undefined && <ErrorBanner className={css.activityError}>{error}</ErrorBanner>}
      {items.length === 0 ? <EmptyState title={t('activity.empty')} /> : rows}
    </section>
  )
  const dockChannel = dock === undefined ? undefined : state.channels.find((c) => c.id === dock.channelId)
  const detailPane = dock === undefined ? null : (
    <section className={css.activityDetailCol} aria-label={dockChannel?.name ?? t('activity.title')}>
      <div className={css.mobileDetailHeader}>
        <PanelHeader title={t('activity.title')} backLabel={t('activity.back')} onBack={closeDock} />
      </div>
      <div className={css.activityDetailPane} key={dockKey}>
        {dock.threadRootId !== undefined && dockThread !== undefined ? (
          <ThreadPanel
            t={t}
            store={store}
            state={state}
            thread={dockThread}
            parentChannelId={dock.channelId}
            activeLocale={activeLocale}
            onRootJump={() => { setDock({ channelId: dock.channelId }) }}
            onClose={() => { setDock({ channelId: dock.channelId }) }}
          />
        ) : dockChannel !== undefined ? (
          <ChannelView
            t={t}
            store={store}
            state={state}
            channel={dockChannel as NativeTarget}
            activeLocale={activeLocale}
            headerActions={<span className={css.desktopClose}><IconButton label={t('activity.closeDock')} icon={<IconCloseOutline16 size={16} />} onClick={closeDock} /></span>}
          />
        ) : (
          <EmptyState title={t('activity.empty')} />
        )}
      </div>
    </section>
  )
  const desktop = dock === undefined ? listPane : (
    <SplitPane
      id="activity-detail"
      leading={listPane}
      trailing={detailPane}
      separatorLabel={t('activity.resize')}
    />
  )
  return (
    <div className={css.activityView} data-docked={dock === undefined ? undefined : 'true'}>
      <ResponsiveDrilldown
        desktop={desktop}
        list={listPane}
        detail={detailPane}
        detailOpen={dock !== undefined}
      />
    </div>
  )
}
