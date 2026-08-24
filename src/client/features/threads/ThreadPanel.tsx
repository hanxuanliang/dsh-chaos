/**
 * Thread panel content. Its width and responsive placement are owned by the
 * shared SplitPane / ResponsiveDrilldown composition in ChannelChatPane.
 *
 * Anatomy (spec §2.1 ASCII):
 *   ┌ 线程 [×]
 *   ├ root card (grey, full root body, click → jump+flash the root in stream)
 *   ├ replies stream (reuses MessageStream verbatim — it is target-generic)
 *   └ composer (draft per thread persisted in localStorage; Enter to send)
 *
 * Truth constraints honored:
 * - thread.create is idempotent per root message (crates create_thread), so
 *   opening a thread from an unthreaded message is always a "get-or-create".
 * - SSE message_created frames now cover thread targets too, so replies
 *   stream in live.
 * - The root card degrades honestly when the root message is outside the
 *   parent's merged window (spec root 找回) — a grey card naming the count,
 *   never a fabricated body.
 * - Composer deliberately has NO As-task toggle: tasks anchor top-level
 *   channel messages, and thread replies are not top-level.
 */
import type { JSX } from 'react'
import { IconChevronLeftOutline14, IconRightUpOutline14 } from '@deepseek-ai/dsh-client-ui-primitives'
import type { NativeActor, NativeMessage, NativeTarget } from '../../../native.ts'
import type { ChaosTranslate } from '../../locales.ts'
import type { CollabStore, CollabStoreSnapshot } from '../../data/store.ts'
import { MessageStream } from '../messages/MessageStream.tsx'
import { ChannelComposer } from '../channels/ChannelComposer.tsx'
import { avatarSeed } from '../../shared/avatar.ts'
import css from './ThreadPanel.module.css'
import { RootCard } from './RootCard.tsx'
import { IconButton } from '../../shared/ui/index.ts'

export function ThreadPanel({ t, store, state, thread, parentChannelId, activeLocale, onRootJump, onClose, onOpenInChannel, back = false, readOnly = false }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  thread: NativeTarget
  parentChannelId: string
  activeLocale(): string
  /** Root card click: close panel + land the root with the jump flash. */
  onRootJump: (messageId: string) => void
  onClose: () => void
  /** Host elevates this thread to the full channel workspace (Activity dock). */
  onOpenInChannel?: (() => void) | undefined
  back?: boolean | undefined
  readOnly?: boolean | undefined
}): JSX.Element {
  const rootMessage: NativeMessage | undefined = thread.rootMessageId === undefined
    ? undefined
    : state.messagesByChannel[parentChannelId]?.find(m => m.id === thread.rootMessageId)
  const rootAuthor: NativeActor | undefined = rootMessage === undefined
    ? undefined
    : state.actors.find(a => a.id === rootMessage.authorId)
  const handle = rootAuthor?.handle ?? ''
  const seed = avatarSeed(handle, rootAuthor?.displayName ?? handle)
  const rootMentionNames = (() => {
    const names = new Set<string>()
    for (const actor of state.actors) {
      names.add(actor.handle.toLowerCase())
      names.add(actor.displayName.toLowerCase())
    }
    return names
  })()  // names 让 root 卡里的 @提及高亮与主流一致——root 完整渲染也含 markdown。

  const replyCount = thread.rootMessageId === undefined
    ? undefined
    : state.threadSummariesByRoot[thread.rootMessageId]?.replyCount
  return (
    <aside className={css.threadPanel} aria-label={t('thread.title')}>
      {/* Thread marker (cumora ThreadDrawer eyebrow+count grammar): the
          panel declares what it IS, no desktop close affordance — closing is
          re-clicking the message's reply marker (toggle) or the root jump.
          Mobile keeps its full-screen back arrow; the dock gets ↗. */}
      <header className={css.threadHead}>
        {back && <IconButton className={css.threadClose} label={t('thread.close')} icon={<IconChevronLeftOutline14 size={14} />} onClick={onClose} />}
        <span className={css.threadTitle}>{t('thread.title')}</span>
        {replyCount !== undefined && replyCount > 0 && (
          <span className={css.threadCount}>{t('thread.replies', { count: replyCount })}</span>
        )}
        {onOpenInChannel !== undefined && (
          <IconButton className={css.threadOpen} label={t('thread.viewInChannel')} tooltip
            icon={<IconRightUpOutline14 size={14} />} onClick={onOpenInChannel} />
        )}
      </header>
      {thread.rootMessageId !== undefined && (
        <RootCard
          t={t}
          rootMessage={rootMessage as NativeMessage | undefined}
          rootAuthor={rootAuthor}
          seed={seed}
          mentionNames={rootMentionNames}
          onJump={(id) => { onRootJump(id) }}
        />
      )}
      <div className={css.threadBody}>
        <MessageStream
          t={t}
          store={store}
          state={state}
          channelId={thread.id}
          activeLocale={activeLocale}
          onOpenTasks={() => { /* threads have no board */ }}
        />
      </div>
      {/* Same seat padding as the channel main column — one placement rule, zero visual drift. */}
      {!readOnly && <div className={css.composerSeat}>
        <ChannelComposer
          t={t}
          store={store}
          state={state}
          channel={thread}
          parentChannelId={parentChannelId}
          disabled={state.connection !== 'live'}
          hideAsTask
        />
      </div>}
    </aside>
  )
}
