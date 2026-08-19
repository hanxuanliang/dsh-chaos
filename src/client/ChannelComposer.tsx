/**
 * In-panel channel composer (spec §5.5 v4): pilot InputBar look (rounded card,
 * auto-growing textarea, 28px solid send button) with plocal-web ChatComposer
 * logic (controlled textarea, per-channel localStorage draft, Plan-B @mention
 * scan + bottom-anchored popover with plain-text `@handle ` serialization,
 * As-Task checkbox + Cmd/Ctrl+Shift+Enter). chaos-kernel mapping: send =
 * message.send (requestId-idempotent, retries reuse the same id), As-Task =
 * message.send then task.create(messageId) as a second step (task.create is
 * idempotent; a failed second step retries that call alone). No model
 * selector / workspace chip / context strip.
 */
import { useEffect, useMemo, useRef, useState, type JSX, type KeyboardEvent } from 'react'
import type { NativeActor, NativeTarget } from '../native.ts'
import { avatarSeed } from './avatar.ts'
import type { CollabStore, CollabStoreSnapshot } from './collab-store.ts'
import type { ChaosTranslate } from './locales.ts'
import css from './CollabPanel.module.css'

export interface ChannelComposerProps {
  t: ChaosTranslate
  store: CollabStore
  state: CollabStoreSnapshot
  channel: NativeTarget
  disabled: boolean
}

const MAX_TEXT_HEIGHT = 336
const DRAFT_PREFIX = 'dsh-chaos:draft:'

function readDraft(targetId: string): string {
  try {
    return localStorage.getItem(DRAFT_PREFIX + targetId) ?? ''
  } catch {
    return ''
  }
}

function writeDraft(targetId: string, value: string): void {
  try {
    if (value === '') localStorage.removeItem(DRAFT_PREFIX + targetId)
    else localStorage.setItem(DRAFT_PREFIX + targetId, value)
  } catch {
    // Storage unavailable (private mode) — drafts simply don't persist.
  }
}

interface MentionState {
  /** Index of the '@' character inside the current text. */
  start: number
  fragment: string
  items: NativeActor[]
  highlight: number
}

type SendFailure = { kind: 'send'; asTask: boolean } | { kind: 'task'; messageId: string }

/** Plan-B token: '@' after a non-token char (CJK/whitespace/punctuation terminate), slug chars only to the caret. */
const MENTION_TOKEN = /(^|[^A-Za-z0-9_@-])@([A-Za-z0-9_-]*)$/

export function ChannelComposer({ t, store, state, channel, disabled }: ChannelComposerProps): JSX.Element {
  const [text, setText] = useState(() => readDraft(channel.id))
  const [asTask, setAsTask] = useState(false)
  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<SendFailure | null>(null)
  const [mention, setMention] = useState<MentionState | null>(null)
  const textareaRef = useRef<HTMLTextAreaElement | null>(null)
  const attemptRef = useRef<{ text: string; requestId: string } | null>(null)
  const draftChannelRef = useRef(channel.id)

  const mentionAgents = useMemo(() => {
    const members = state.membersByChannel[channel.id] ?? state.actors
    return members.filter(actor => actor.kind === 'agent')
  }, [state.membersByChannel, state.actors, channel.id])

  // Switching channels restores that channel's draft and resets As-Task and
  // any stale attempt, exactly like plocal's target switch.
  useEffect(() => {
    setText(readDraft(channel.id))
    setAsTask(false)
    setFailure(null)
    setMention(null)
    attemptRef.current = null
  }, [channel.id])

  useEffect(() => {
    if (draftChannelRef.current !== channel.id) {
      draftChannelRef.current = channel.id
      return
    }
    writeDraft(channel.id, text)
  }, [channel.id, text])

  // Auto-grow (pilot mirror-div behavior minus the overlay layers): resize the
  // native textarea from scrollHeight and cap growth at the 336px text axis.
  useEffect(() => {
    const el = textareaRef.current
    if (el === null) return
    el.style.height = 'auto'
    el.style.height = `${String(Math.min(el.scrollHeight, MAX_TEXT_HEIGHT))}px`
    el.style.overflowY = el.scrollHeight > MAX_TEXT_HEIGHT ? 'auto' : 'hidden'
  }, [text, channel.id])

  const refreshMention = (el: HTMLTextAreaElement): void => {
    const caret = el.selectionStart
    if (caret !== el.selectionEnd) {
      setMention(null)
      return
    }
    const match = MENTION_TOKEN.exec(el.value.slice(0, caret))
    if (match === null) {
      setMention(null)
      return
    }
    const fragment = match[2] ?? ''
    const lower = fragment.toLowerCase()
    const items = mentionAgents
      .filter(agent => lower === ''
        || agent.handle.toLowerCase().includes(lower)
        || agent.displayName.toLowerCase().includes(lower))
      .slice(0, 6)
    if (items.length === 0) {
      setMention(null)
      return
    }
    setMention({ start: caret - fragment.length - 1, fragment, items, highlight: 0 })
  }

  const pickMention = (item: NativeActor): void => {
    const el = textareaRef.current
    const current = mention
    setMention(null)
    if (el === null || current === null) return
    const inserted = `@${item.handle} `
    setText(text.slice(0, current.start) + inserted + text.slice(el.selectionStart))
    const caret = current.start + inserted.length
    el.focus()
    requestAnimationFrame(() => { el.setSelectionRange(caret, caret) })
  }

  const executeSend = async (forceAsTask: boolean): Promise<void> => {
    const body = text.trim()
    if (body === '' || busy || disabled) return
    // One logical attempt = one requestId; retrying the same body reuses it,
    // and the backend's UNIQUE(author, client_request_id) keeps that safe.
    const attempt = attemptRef.current !== null && attemptRef.current.text === body
      ? attemptRef.current
      : { text: body, requestId: crypto.randomUUID() }
    attemptRef.current = attempt
    setBusy(true)
    setFailure(null)
    try {
      const message = await store.sendMessage(channel.id, attempt.requestId, body)
      attemptRef.current = null
      // The message already landed; a failed As-Task second step must never
      // resurrect the text (retry then re-fires task.create alone).
      setText('')
      writeDraft(channel.id, '')
      if (forceAsTask) {
        try {
          await store.createTask(message.id)
        } catch {
          setFailure({ kind: 'task', messageId: message.id })
        }
      }
    } catch {
      setFailure({ kind: 'send', asTask: forceAsTask })
    }
    setBusy(false)
    textareaRef.current?.focus()
  }

  const retry = (): void => {
    if (failure === null || busy || disabled) return
    if (failure.kind === 'send') {
      void executeSend(failure.asTask)
      return
    }
    setBusy(true)
    store.createTask(failure.messageId).then(() => {
      setFailure(null)
      setBusy(false)
    }, () => {
      // Keep the same inline failure; the click did not clear it.
      setBusy(false)
    })
  }

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (mention !== null) {
      const step = event.key === 'ArrowDown' ? 1 : mention.items.length - 1
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        setMention(current => current === null ? null : {
          ...current,
          highlight: (current.highlight + step) % current.items.length,
        })
        return
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        event.preventDefault()
        const item = mention.items[mention.highlight]
        if (item !== undefined) pickMention(item)
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        setMention(null)
        return
      }
    }
    if (event.key !== 'Enter') return
    if ((event.metaKey || event.ctrlKey) && event.shiftKey) {
      event.preventDefault()
      void executeSend(true)
      return
    }
    if (event.shiftKey) return
    // IME composing (Safari's late compositionend covered by keyCode 229).
    if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return
    event.preventDefault()
    void executeSend(asTask)
  }

  const inert = busy || disabled

  return (
    <div className={css.composerWrap}>
      {failure !== null && (
        <div className={css.composerError} role="alert">
          <span>{failure.kind === 'send' ? t('composer.sendFailed') : t('composer.taskFailed')}</span>
          <button type="button" className={css.retryButton} disabled={busy} onClick={retry}>
            {t('composer.retry')}
          </button>
        </div>
      )}
      <div className={css.composer} data-inert={inert || undefined}>
        {mention !== null && (
          <div className={css.mentionPop} role="listbox">
            {mention.items.map((agent, index) => {
              const seed = avatarSeed(agent.handle, agent.displayName)
              return (
                <button
                  key={agent.id}
                  type="button"
                  role="option"
                  aria-selected={index === mention.highlight}
                  data-active={index === mention.highlight || undefined}
                  className={css.mentionItem}
                  onMouseDown={(event) => { event.preventDefault() }}
                  onMouseEnter={() => {
                    setMention(current => current === null ? null : { ...current, highlight: index })
                  }}
                  onClick={() => { pickMention(agent) }}
                >
                  <span className={css.avatarXs} style={{ background: seed.background }} aria-hidden="true">{seed.initial}</span>
                  <span className={css.memberName}>{agent.displayName}</span>
                  <span className={css.memberHandle}>@{agent.handle}</span>
                </button>
              )
            })}
          </div>
        )}
        <div className={css.composerScroll}>
          <textarea
            ref={textareaRef}
            className={css.composerInput}
            rows={2}
            value={text}
            placeholder={t('composer.placeholder', { name: channel.name })}
            disabled={inert}
            onChange={(event) => { setText(event.target.value); refreshMention(event.currentTarget) }}
            onKeyDown={onKeyDown}
            onClick={(event) => { refreshMention(event.currentTarget) }}
            onKeyUp={(event) => {
              const passive = ['Enter', 'Tab', 'Escape', 'ArrowDown', 'ArrowUp']
              if (!passive.includes(event.key)) refreshMention(event.currentTarget)
            }}
          />
        </div>
        <div className={css.composerRow}>
          <label className={css.asTask}>
            <input
              type="checkbox"
              checked={asTask}
              disabled={inert}
              onChange={(event) => { setAsTask(event.target.checked) }}
            />
            <span>{t('composer.asTask')}</span>
          </label>
          <button
            type="button"
            className={css.sendButton}
            aria-label={t('composer.send')}
            title={t('composer.send')}
            disabled={inert || text.trim() === ''}
            onClick={() => { void executeSend(asTask) }}
          >
            <svg viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M8 12.5v-9M4.5 7L8 3.5 11.5 7" />
            </svg>
          </button>
        </div>
      </div>
    </div>
  )
}
