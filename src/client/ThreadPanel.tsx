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
import { useEffect, useRef, useState, type JSX } from 'react'
import type { NativeActor, NativeMessage, NativeTarget } from '../native.ts'
import type { ChaosTranslate } from './locales.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import { MessageStream } from './MessageStream.tsx'
import { avatarSeed } from './avatar.ts'
import css from './CollabPanel.module.css'

function readThreadDraft(threadId: string): string {
  try { return window.localStorage.getItem(`dsh-chaos:draft:thread:${threadId}`) ?? '' } catch { return '' }
}
function writeThreadDraft(threadId: string, value: string): void {
  try {
    if (value === '') window.localStorage.removeItem(`dsh-chaos:draft:thread:${threadId}`)
    else window.localStorage.setItem(`dsh-chaos:draft:thread:${threadId}`, value)
  } catch { /* storage may be unavailable; drafts are best-effort */ }
}

function ThreadComposer({ t, store, threadId }: {
  t: ChaosTranslate
  store: CollabStore
  threadId: string
}): JSX.Element {
  const [text, setText] = useState(() => readThreadDraft(threadId))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | undefined>(undefined)
  const requestRef = useRef<string>(crypto.randomUUID())

  useEffect(() => {
    setText(readThreadDraft(threadId))
    setError(undefined)
    requestRef.current = crypto.randomUUID()
  }, [threadId])

  const send = async (): Promise<void> => {
    const body = text.trim()
    if (body === '' || busy) return
    setBusy(true)
    setError(undefined)
    try {
      await store.sendMessage(threadId, requestRef.current, body)
      setText('')
      writeThreadDraft(threadId, '')
      requestRef.current = crypto.randomUUID()
    } catch (cause) {
      setError(t('thread.sendFailed', { error: cause instanceof Error ? cause.message : String(cause) }))
    }
    setBusy(false)
  }

  return (
    <div className={css.threadComposer}>
      {error !== undefined && <div className={css.threadComposerError} role="alert">{error}</div>}
      <textarea
        className={css.threadComposerInput}
        rows={2}
        placeholder={t('thread.placeholder')}
        aria-label={t('thread.placeholder')}
        value={text}
        disabled={busy}
        onChange={(event) => { setText(event.target.value); writeThreadDraft(threadId, event.target.value) }}
        onKeyDown={(event) => {
          if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return
          if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void send() }
        }}
      />
      <button
        type="button"
        className={css.threadComposerSend}
        disabled={busy || text.trim() === ''}
        aria-label={t('composer.send')}
        onClick={() => { void send() }}
      >
        <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M8 13V3M4 6.5 8 2.5l4 4" />
        </svg>
      </button>
    </div>
  )
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

  return (
    <aside className={css.threadPanel} aria-label={t('thread.title')}>
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
            <span className={css.threadRootText}>{(rootMessage as NativeMessage).text}</span>
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
      <ThreadComposer t={t} store={store} threadId={thread.id} />
    </aside>
  )
}
