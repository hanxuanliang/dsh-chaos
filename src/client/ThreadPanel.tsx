/**
 * Thread panel (P0-5) — spec §2.1: 360px full-height right column taking over
 * the main area's right side whenever a thread is open.
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
import { useCallback, useEffect, useRef, useState, type JSX } from 'react'
import type { NativeActor, NativeMessage, NativeTarget } from '../native.ts'
import type { ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import { MessageBody, MessageStream } from './MessageStream.tsx'
import { ChannelComposer } from './ChannelComposer.tsx'
import { avatarSeed } from './avatar.ts'
import css from './CollabPanel.module.css'

const THREAD_WIDTH_KEY = 'dsh-chaos:threadPanelWidth'
const THREAD_WIDTH_MIN = 340
const THREAD_WIDTH_MAX = 900

function readThreadWidth(): number {
  try {
    const raw = window.localStorage.getItem(THREAD_WIDTH_KEY)
    const parsed = raw === null ? NaN : Number.parseInt(raw, 10)
    return Number.isFinite(parsed) ? Math.min(THREAD_WIDTH_MAX, Math.max(THREAD_WIDTH_MIN, parsed)) : 480
  } catch { return 360 }
}

export function ThreadPanel({ t, store, state, thread, parentChannelId, activeLocale, onRootJump, onClose }: {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  thread: NativeTarget
  parentChannelId: string
  activeLocale(): string
  /** Root card click: close panel + land the root with the jump flash. */
  onRootJump: (messageId: string) => void
  onClose: () => void
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

  const [width, setWidth] = useState(readThreadWidth)
  const [dragging, setDragging] = useState(false)
  const dragStartX = useRef(0)
  const dragStartWidth = useRef(width)

  /** 左缘分隔条拖拽：plocal 现象用户反馈——thread 太小时必须可拉。 */
  const startDrag = useCallback((clientX: number) => {
    dragStartX.current = clientX
    dragStartWidth.current = width
    setDragging(true)
  }, [width])

  useEffect(() => {
    if (!dragging) return
    const onMove = (event: MouseEvent): void => {
      const next = dragStartWidth.current + (dragStartX.current - event.clientX)
      setWidth(Math.min(THREAD_WIDTH_MAX, Math.max(THREAD_WIDTH_MIN, next)))
    }
    const onUp = (): void => {
      setDragging(false)
      setWidth(current => {
        try { window.localStorage.setItem(THREAD_WIDTH_KEY, String(current)) } catch { /* best-effort */ }
        return current
      })
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
    return () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
  }, [dragging])

  return (
    <aside className={css.threadPanel} style={{ position: 'relative', width }} aria-label={t('thread.title')}>
      <div
        className={css.threadResizeHandle}
        data-dragging={dragging || undefined}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize thread panel"
        onMouseDown={(event) => { event.preventDefault(); startDrag(event.clientX) }}
      />
      <header className={css.threadHead}>
        <span className={css.threadTitle}>{t('thread.title')}</span>
        <button type="button" className={css.threadClose} aria-label={t('thread.close')} onClick={onClose}>
          <svg viewBox="0 0 16 16" width="11" height="11" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" aria-hidden="true">
            <path d="m4 4 8 8M12 4l-8 8" />
          </svg>
        </button>
      </header>
      {thread.rootMessageId !== undefined && (
        rootMessage !== undefined ? (
          <button
            type="button"
            className={`${css.threadRoot} ${css.threadRootLink}`}
            title={t('thread.rootJump')}
            onClick={() => { onRootJump((rootMessage as NativeMessage).id) }}
          >
            <span className={css.threadRootHead}>
              <span className={css.avatarXs} style={{ background: seed.background }} aria-hidden="true">{seed.initial}</span>
              <span className={css.threadRootAuthor}>{rootAuthor?.displayName ?? handle}</span>
            </span>
            <div className={css.threadRootText}>
              <MessageBody t={t} text={(rootMessage as NativeMessage).text} names={rootMentionNames} />
            </div>
          </button>
        ) : (
          <div className={css.threadRoot} data-missing="true">
            <span className={css.threadRootText}>{t('thread.rootMissing')}</span>
          </div>
        )
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
      <div className={css.composerSeat}>
        <ChannelComposer t={t} store={store} state={state} channel={thread} disabled={state.connection !== 'live'} hideAsTask />
      </div>
    </aside>
  )
}
