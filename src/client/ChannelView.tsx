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
  useEffect(() => { setTab('messages') }, [channel.id])

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
        {members !== undefined && (
          <span className={css.channelMeta}>{t('channel.members', { count: members.length })}</span>
        )}
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
          <div className={css.mainEmpty}>
            <p className={css.empty}>{t('channel.tasksPending')}</p>
          </div>
        )}
    </section>
  )
}
