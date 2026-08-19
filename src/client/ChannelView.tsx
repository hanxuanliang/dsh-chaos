/**
 * Active-channel container (spec §1.2): 44px header (# name + member count
 * meta + 「消息 | 任务 (N)」tab switch, N = non-done tasks), message stream,
 * and the in-panel composer. The tasks tab is a P0-2 placeholder only — the
 * real board lands with P0-4.
 */
import { useEffect, useState, type JSX } from 'react'
import type { NativeTarget } from '../native.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import type { ChaosTranslate } from './locales.ts'
import { MessageStream } from './MessageStream.tsx'
import { ChannelComposer } from './ChannelComposer.tsx'
import { ChannelMembersDialog } from './ChannelMembersDialog.tsx'
import { ChannelTasksBoard } from './ChannelTasksBoard.tsx'
import css from './CollabPanel.module.css'

export interface ChannelViewProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channel: NativeTarget
  activeLocale(): string
}

export function ChannelView({ t, store, state, channel, activeLocale }: ChannelViewProps): JSX.Element {
  const [tab, setTab] = useState<'messages' | 'tasks'>('messages')
  const [membersOpen, setMembersOpen] = useState(false)
  useEffect(() => { setTab('messages') }, [channel.id])
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
          <>
            <MessageStream
              t={t}
              store={store}
              state={state}
              channelId={channel.id}
              activeLocale={activeLocale}
              onOpenTasks={() => { setTab('tasks') }}
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
          </>
        )
        : (
          <ChannelTasksBoard t={t} store={store} state={state} channelId={channel.id} />
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
