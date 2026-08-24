/**
 * Active-channel container: compact header (# name + member count + message /
 * task tabs), message stream, task board, and the official-style composer.
 */
import type { ReactNode } from 'react'
import { useEffect, useRef, useState, type JSX } from 'react'
import { IconUserOutline16 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeTarget } from '../../../native.ts'
import type { CollabStore, CollabStoreSnapshot } from '../../data/store.ts'
import type { ChaosTranslate } from '../../locales.ts'
import { MessageStream } from '../messages/MessageStream.tsx'
import { ChannelComposer } from './ChannelComposer.tsx'
import { ChannelMembersDialog } from './ChannelMembersDialog.tsx'
import { ChannelTasksBoard } from '../tasks/ChannelTasksBoard.tsx'
import { ThreadPanel } from '../threads/ThreadPanel.tsx'
import css from './ChannelView.module.css'
import { Tabs } from '../../shared/ui/index.ts'
import { Button } from '@deepseek-ai/dsh-client-ui-primitives'
import { ResponsiveDrilldown, SplitPane } from '../../shared/layout/index.ts'

export interface ChannelViewProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channel: NativeTarget
  activeLocale(): string
  /**
   * Activity 行点跨频道 thread 的一次性入口: mount 时经 useState 初始
   * 值消费(不允许 effect/setState 接力——教训实证 2026-08-20)。
   */
  pendingThreadRoot?: string | undefined
  /** 跨层消耗回调: Parent(mount 消费者) 需要烤定 pending state; Activity dock 无此链路。 */
  onPendingThreadConsumed?(): void
  /** Optional host-surface actions rendered in the Channel header action row. */
  headerActions?: ReactNode
  /** Optional leading navigation rendered before the Channel title. */
  headerLeading?: ReactNode
  /** Start the shared Agent creation flow from the member picker. */
  onCreateAgent(channelId: string): void
  /** 深度提升: channel 区 thread 面板头的 ↗ (宿主注入, 无则不渲)。 */
  onThreadOpenInChannel?(): void
}

export function ChannelView({ t, store, state, channel, activeLocale, pendingThreadRoot, onPendingThreadConsumed, headerActions, headerLeading, onCreateAgent, onThreadOpenInChannel }: ChannelViewProps): JSX.Element {
  const [tab, setTab] = useState<'messages' | 'tasks'>('messages')
  const [membersOpen, setMembersOpen] = useState(false)
  /** One-shot jump request: task anchor click → land on the stream row. */
  const [jumpMessageId, setJumpMessageId] = useState<string | undefined>(undefined)
  /** One-shot inverse jump: message Task chip → reveal that exact board card. */
  const [focusedTaskMessageId, setFocusedTaskMessageId] = useState<string | undefined>(undefined)
  /** Open thread root —— 初始值吃 pendingThreadRoot mount 时一次性消费。 */
  const [threadRootId, setThreadRootId] = useState<string | undefined>(pendingThreadRoot ?? undefined)
  // mount 初始值已吃 pendingThreadRoot; 这个 effect 只管「同频道 lifetime
  // 内」的后续消费(mount 时 threadRootId 已被初始值顶着, 不会双重开火)。
  useEffect(() => {
    if (pendingThreadRoot === undefined || threadRootId === pendingThreadRoot) return
    setThreadRootId(pendingThreadRoot)
    setJumpMessageId(pendingThreadRoot)
    onPendingThreadConsumed?.()
  }, [pendingThreadRoot, threadRootId, onPendingThreadConsumed])
  const thread = threadRootId === undefined
    ? undefined
    : state.threads.find(t => t.rootMessageId === threadRootId)
  const threadOpeningRef = useRef<string | undefined>(undefined)
  useEffect(() => {
    if (threadRootId === undefined) return
    if (thread !== undefined && state.messagesByChannel[thread.id] !== undefined) return
    if (threadOpeningRef.current === threadRootId) return
    threadOpeningRef.current = threadRootId
    void store.openThread(threadRootId).catch(() => {
      threadOpeningRef.current = undefined
    })
  }, [threadRootId, thread, state.messagesByChannel, store])
  // (无重置 effect: ChannelView 以 channel.id 作 key 整树重挂, mount effect
  // 会误伤 useState 初始化器——此前的 threadRootId 曾被这样子抹掉两次。)
  useEffect(() => { setMembersOpen(false) }, [channel.id])

  const members = state.membersByChannel[channel.id]
  const openTasks = Object.values(state.tasksByMessage)
    .filter(task => task.targetId === channel.id && task.status !== 'done').length
  const readOnly = channel.lifecycle === 'archived'
  const composerDisabled = state.connection !== 'live'

  // 头部元在 messages 模式落进主列 mainCol (与 thread 水平; rc 2026-08-20), 在
  // tasks 模式满铺 (无 thread 并存)。
  const channelHead = (
    <header className={css.channelHead}>
      {headerLeading}
      <h3 className={css.channelTitle}>
        <span className={css.channelHash} aria-hidden="true">#</span>
        {channel.name}
      </h3>
      <Tabs<'messages' | 'tasks'>
        variant="panel"
        value={tab}
        onValueChange={setTab}
        label={t('panel.title')}
        items={[
          { id: 'messages', label: t('channel.tabMessages') },
          { id: 'tasks', label: t('channel.tabTasks', { count: openTasks }) },
        ]}
      />
      {members !== undefined && (
        <Button
          className={css.memberChip}
          variant="ghost"
          size="sm"
          icon={<IconUserOutline16 size={14} />}
          title={t('channel.membersLabel')}
          aria-label={t('channel.membersLabel')}
          aria-haspopup="dialog"
          onClick={() => { setMembersOpen(true) }}
        >
          <span className={css.memberCount}>{members.length}</span>
        </Button>
      )}
      {headerActions}
    </header>
  )

  return (
    <section className={css.channel} aria-label={`# ${channel.name}`}>
      {tab === 'tasks' && channelHead}
      {tab === 'messages'
        ? (
          <ChannelChatPane
            t={t}
            store={store}
            state={state}
            channel={channel}
            channelHead={channelHead}
            thread={threadRootId !== undefined ? thread : undefined}
            jumpMessageId={jumpMessageId}
            onJumpHandled={() => { setJumpMessageId(undefined) }}
            activeLocale={activeLocale}
            onOpenTasks={(messageId) => { setFocusedTaskMessageId(messageId); setTab('tasks') }}
            onOpenThread={(messageId) => { setThreadRootId(cur => cur === messageId ? undefined : messageId) }}
            onCloseThread={() => { setThreadRootId(undefined) }}
            onRootJump={(messageId) => { setThreadRootId(undefined); setJumpMessageId(messageId) }}
            onThreadOpenInChannel={onThreadOpenInChannel === undefined
          ? undefined
          : () => {
              /* 已在频道工作面: ↗ = 收起面板 + 锚定主流 root 行 (jump-flash)。 */
              const root = threadRootId
              setThreadRootId(undefined)
              if (root !== undefined) setJumpMessageId(root)
              onThreadOpenInChannel?.()
            }}
            composerDisabled={composerDisabled}
          />
        )
        : (
          <ChannelTasksBoard
            t={t}
            store={store}
            state={state}
            channelId={channel.id}
            focusMessageId={focusedTaskMessageId}
            readOnly={readOnly}
            onFocusHandled={() => { setFocusedTaskMessageId(undefined) }}
            onOpenMessage={(messageId) => { setTab('messages'); setJumpMessageId(messageId) }}
          />
        )}
      {membersOpen && (
        <ChannelMembersDialog
          t={t}
          store={store}
          state={state}
          channelId={channel.id}
          readOnly={readOnly}
          onCreateAgent={() => { setMembersOpen(false); onCreateAgent(channel.id) }}
          onClose={() => { setMembersOpen(false) }}
        />
      )}
    </section>
  )
}


/**
 * ChannelChatPane — channel 内容区唯一来源(plocal shell main 对照):
 * [ .channelMainRow: SplitPane(.channelMainCol | ThreadPanel)? ]
 * 「先展示 channel, 有 thread 才在右侧展开」。ChannelView messages tab 自己用
 * 它; Activity 右栏 dock 也用同一组件, 不再手拼第二套。
 */
export function ChannelChatPane({ t, store, state, channel, channelHead, thread, jumpMessageId, onJumpHandled, activeLocale, onOpenTasks, onOpenThread, onCloseThread, onRootJump, onThreadOpenInChannel, composerDisabled }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channel: NativeTarget
  /** 头部元 — rc 2026-08-20 messages 模式落主列; **undefined**=不渲头(Activity dock 处于也别给) */
  channelHead: ReactNode | undefined
  thread: NativeTarget | undefined
  jumpMessageId: string | undefined
  onJumpHandled(): void
  activeLocale(): string
  onOpenTasks(messageId: string): void
  onOpenThread(messageId: string): void
  onCloseThread(): void
  onRootJump(messageId: string): void
  onThreadOpenInChannel?: (() => void) | undefined
  composerDisabled: boolean
}): JSX.Element {
  const readOnly = channel.lifecycle === 'archived'
  const mainPane = (
    <div className={css.channelMainCol}>
        {channelHead}
        <MessageStream jumpMessageId={jumpMessageId} onJumpHandled={onJumpHandled}
          t={t}
          store={store}
          state={state}
          channelId={channel.id}
          readOnly={readOnly}
          activeLocale={activeLocale}
          onOpenTasks={onOpenTasks}
          onOpenThread={onOpenThread}
        />
        {/* The composer hangs on the same centered 780px column as the stream. */}
        {!readOnly && <div className={css.composerSeat}>
          <ChannelComposer
            t={t}
            store={store}
            state={state}
            channel={channel}
            disabled={composerDisabled}
          />
        </div>}
    </div>
  )
  if (thread === undefined) return <div className={css.channelMainRow}>{mainPane}</div>

  const threadPane = (
    <ThreadPanel
      t={t}
      store={store}
      state={state}
      thread={thread}
      parentChannelId={channel.id}
      activeLocale={activeLocale}
      onRootJump={onRootJump}
      onClose={onCloseThread}
      onOpenInChannel={onThreadOpenInChannel}
      readOnly={readOnly}
    />
  )
  const mobileThreadPane = (
    <ThreadPanel
      t={t}
      store={store}
      state={state}
      thread={thread}
      parentChannelId={channel.id}
      activeLocale={activeLocale}
      onRootJump={onRootJump}
      onClose={onCloseThread}
      onOpenInChannel={onThreadOpenInChannel}
      back
      readOnly={readOnly}
    />
  )
  return (
    <div className={css.channelMainRow}>
      <ResponsiveDrilldown
        desktop={(
          <SplitPane
            id={`channel-thread:${channel.id}`}
            leading={mainPane}
            trailing={threadPane}
            fixedSide="trailing"
            leadingMin={220}
            trailingDefault={360}
            trailingMin={300}
            trailingMax="66.667%"
            separatorLabel={t('thread.resize')}
          />
        )}
        list={mainPane}
        detail={mobileThreadPane}
        detailOpen
        breakpoint={640}
      />
    </div>
  )
}
