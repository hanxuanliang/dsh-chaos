/**
 * Activity 视图(rc-4 统一性整改):
 * - 行卡: 目标小行 → 主标题(粗) → 最新回复 → 底行(task chip+N replies);
 *   不再有前置 kind 图标(点击自然开右栏)。
 * - 点击 = 列表收窄 + 右栏 dock,channel/thread 同一套 dock 机制, 不再整页跳:
 *   channel → DockedChannelPane(header-lite + MessageStream + ChannelComposer);
 *   thread → ThreadPanel 本体(与频道内同一件)。
 * - 组件复用: Button pill 与频道 tab 同一 css.tab 族; 状态色块 = StatusChip
 *   (与看板 toggle 同源); composer = ChannelComposer; 流 = MessageStream。
 * - 筛选 pill: All 生效; Unread/Mentions disabled(plocal 此刻同样 disabled —
 *   read vertical 未落地), Mark-all-read 同因 disabled。不做假交互。
 * - direct 行暂不做(DM 主界面没建,点击没有诚实目标 — 隐藏)。
 */
import { useMemo, useState, type JSX } from 'react'
import { IconCloseOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActivityInboxItem, NativeTarget } from '../native.ts'
import css from './blocks/ActivityView.module.css'
import type { ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import { ChannelView } from './ChannelView.tsx'
import { ThreadPanel } from './ThreadPanel.tsx'
import { PillTabs } from './atoms/PillTabs.tsx'
import { ActivityCard } from './blocks/ActivityCard.tsx'
import cardCss from './blocks/ActivityCard.module.css'
import { ErrorBanner } from './atoms/ErrorBanner.tsx'
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
  const open = (item: NativeActivityInboxItem): void => {
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
  const filterTabs = (
    <PillTabs
      align="lead"
      ariaLabel={t('activity.filtersAria')}
      items={[
        { id: 'all', label: t('activity.filterAll'), active: true },
        { id: 'unread', label: t('activity.filterUnread'), disabled: true, title: t('activity.filterPending') },
        { id: 'mentions', label: t('activity.filterMentions'), disabled: true, title: t('activity.filterPending') },
      ]}
    />
  )
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
          onOpen={() => { open(item) }}
          onDone={() => { markDone(item) }}
        />
      ))}
    </div>
 )
  const headerRow = (
    <header className={css.activityHeader}>
      <h2 className={css.activityTitle}>{t('activity.title')}</h2>
      <div className={css.activityHeadTabs}>{filterTabs}</div>
    </header>
  )
  if (dock !== undefined) {
    // dock 态 = 两列: 左列 [Activity 头 + 筛行 + 列表全栈]; 右列 [dock 头 + ChannelChatPane body]
    const dockChannel = state.channels.find((c) => c.id === dock.channelId)
    return (
      <div className={css.activityView} data-docked="true">
        <div className={css.activityListCol}>
          {headerRow}
          {error !== undefined && <ErrorBanner>{error}</ErrorBanner>}
          {rows}
        </div>
        <div className={css.activityDetailCol}>
          <div className={css.activityDetailPane} key={dockKey}>
            {/* ① channel 行: 右区 = ChannelView 自体(完全铺开, thread 只在里面点了才开)。
                ② thread 行: 右区 = ThreadPanel 自体平铺(thread 页面一份, 不要 channel 夹带)。 */}
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
            ) : (
              <ChannelView
                t={t}
                store={store}
                state={state}
                channel={dockChannel as NativeTarget}
                activeLocale={activeLocale}
              />
            )}
            <button type="button" className={css.activityDockClose} aria-label={t('activity.closeDock')} title={t('activity.closeDock')} onClick={() => { setDock(undefined) }}>
              <IconCloseOutline16 />
            </button>
          </div>
        </div>
      </div>
    )
  }
  return (
    <div className={css.activityView}>
      {headerRow}
      {error !== undefined && <ErrorBanner>{error}</ErrorBanner>}
      {items.length === 0 ? <div className={css.activityEmpty}>{t('activity.empty')}</div> : rows}
    </div>
  )
}
