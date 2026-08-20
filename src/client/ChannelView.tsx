/**
 * Active-channel container (spec §1.2): 44px header (# name + member count
 * meta + 「消息 | 任务 (N)」tab switch, N = non-done tasks), message stream,
 * and the in-panel composer. The tasks tab is a P0-2 placeholder only — the
 * real board lands with P0-4.
 */
import { useEffect, useRef, useState, type JSX } from 'react'
import type { NativeTarget } from '../native.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import type { ChaosTranslate } from './locales.ts'
import { MessageStream } from './MessageStream.tsx'
import { ChannelComposer } from './ChannelComposer.tsx'
import { ChannelMembersDialog } from './ChannelMembersDialog.tsx'
import { ChannelTasksBoard } from './ChannelTasksBoard.tsx'
import { ThreadPanel } from './ThreadPanel.tsx'
import css from './CollabPanel.module.css'

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
  onPendingThreadConsumed(): void
}

export function ChannelView({ t, store, state, channel, activeLocale, pendingThreadRoot, onPendingThreadConsumed }: ChannelViewProps): JSX.Element {
  const [tab, setTab] = useState<'messages' | 'tasks' | 'activity'>('messages')
  const [membersOpen, setMembersOpen] = useState(false)
  /** One-shot jump request: task anchor click → land on the stream row. */
  const [jumpMessageId, setJumpMessageId] = useState<string | undefined>(undefined)
  /** Open thread root —— 初始值吃 pendingThreadRoot mount 时一次性消费。 */
  const [threadRootId, setThreadRootId] = useState<string | undefined>(pendingThreadRoot ?? undefined)
  // mount 初始值已吃 pendingThreadRoot; 这个 effect 只管「同频道 lifetime
  // 内」的后续消费(mount 时 threadRootId 已被初始值顶着, 不会双重开火)。
  useEffect(() => {
    if (pendingThreadRoot === undefined || threadRootId === pendingThreadRoot) return
    setThreadRootId(pendingThreadRoot)
    setJumpMessageId(pendingThreadRoot)
    onPendingThreadConsumed()
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
  const composerDisabled = state.connection !== 'live'

  return (
    <section className={css.channel} aria-label={`# ${channel.name}`}>
      <header className={css.channelHead}>
        <h3 className={css.channelTitle}>
          <span className={css.channelHash} aria-hidden="true">#</span>
          {channel.name}
        </h3>
        <div className={css.tabs} role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={tab === 'messages'}
            data-active={tab === 'messages' || undefined}
            className={css.tab}
            onClick={() => { setTab('messages') }}
          >
            {t('channel.tabMessages')}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === 'tasks'}
            data-active={tab === 'tasks' || undefined}
            className={css.tab}
            onClick={() => { setTab('tasks') }}
          >
            {t('channel.tabTasks', { count: openTasks })}
          </button>
        </div>
        {/* 成员数 chip 单独挂在头部最右端（用户 2026-08-19 拍板），不与 tab 组并列 */}
        {members !== undefined && (
          <button
            type="button"
            className={css.memberChip}
            aria-haspopup="dialog"
            onClick={() => { setMembersOpen(true) }}
           title={t('channel.membersLabel')} aria-label={t('channel.membersLabel')}>
            <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <circle cx="6" cy="5" r="2.6" />
              <path d="M1.8 13.2c.6-2.4 2.2-3.6 4.2-3.6s3.6 1.2 4.2 3.6" />
              <path d="M10.3 7.6c1.4 0 2.6-1.1 2.6-2.6 0-.4-.1-.8-.2-1.2" />
              <path d="M11.6 9.7c1.4.3 2.4 1.4 2.7 3.5" />
            </svg>
            <span className={css.memberCount}>{members.length}</span>
          </button>
        )}
      </header>
      {tab === 'messages'
        ? (
          <ChannelChatPane
            t={t}
            store={store}
            state={state}
            channel={channel}
            thread={threadRootId !== undefined ? thread : undefined}
            jumpMessageId={jumpMessageId}
            onJumpHandled={() => { setJumpMessageId(undefined) }}
            activeLocale={activeLocale}
            onOpenTasks={() => { setTab('tasks') }}
            onOpenThread={(messageId) => { setThreadRootId(messageId) }}
            onCloseThread={() => { setThreadRootId(undefined) }}
            onRootJump={(messageId) => { setThreadRootId(undefined); setJumpMessageId(messageId) }}
            composerDisabled={composerDisabled}
          />
        )
        : (
          <ChannelTasksBoard
            t={t}
            store={store}
            state={state}
            channelId={channel.id}
            onOpenMessage={(messageId) => { setTab('messages'); setJumpMessageId(messageId) }}
          />
        )}
      {membersOpen && (
        <ChannelMembersDialog
          t={t}
          store={store}
          state={state}
          channelId={channel.id}
          onClose={() => { setMembersOpen(false) }}
        />
      )}
    </section>
  )
}


/**
 * ChannelChatPane — channel 内容区唯一来源(plocal shell main 对照):
 * [ .channelMainRow: .channelMainCol(MessageStream + composer) | ThreadPanel? ]
 * 「先展示 channel, 有 thread 才在右侧展开」。ChannelView messages tab 自己用
 * 它; Activity 右栏 dock 也用同一组件, 不再手拼第二套。
 */
export function ChannelChatPane({ t, store, state, channel, thread, jumpMessageId, onJumpHandled, activeLocale, onOpenTasks, onOpenThread, onCloseThread, onRootJump, composerDisabled }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channel: NativeTarget
  thread: NativeTarget | undefined
  jumpMessageId: string | undefined
  onJumpHandled(): void
  activeLocale(): string
  onOpenTasks(): void
  onOpenThread(messageId: string): void
  onCloseThread(): void
  onRootJump(messageId: string): void
  composerDisabled: boolean
}): JSX.Element {
  return (
    <div className={css.channelMainRow}>
      <div className={css.channelMainCol}>
        <MessageStream jumpMessageId={jumpMessageId} onJumpHandled={onJumpHandled}
          t={t}
          store={store}
          state={state}
          channelId={channel.id}
          activeLocale={activeLocale}
          onOpenTasks={onOpenTasks}
          onOpenThread={onOpenThread}
        />
        {/* The composer hangs on the same centered 780px column as the stream. */}
        <div className={css.composerSeat}>
          <ChannelComposer
            t={t}
            store={store}
            state={state}
            channel={channel}
            disabled={composerDisabled}
          />
        </div>
      </div>
      {thread !== undefined && (
        <ThreadPanel
          t={t}
          store={store}
          state={state}
          thread={thread}
          parentChannelId={channel.id}
          activeLocale={activeLocale}
          onRootJump={onRootJump}
          onClose={onCloseThread}
        />
      )}
    </div>
  )
}
