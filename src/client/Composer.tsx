import { useEffect, useRef, type KeyboardEvent } from 'react'
import css from './ChaosPanel.module.css'

/**
 * Shared multiline composer (main conversation + Thread panel): the textarea
 * auto-grows up to a cap and then scrolls, Enter sends, Shift+Enter inserts a
 * newline. Sending is guarded: while a send is pending the composer blocks
 * repeat submits, a failure keeps the draft and shows the error, and only a
 * successful send clears it (parent-owned via value/onChange).
 */
export function Composer({
  value,
  onChange,
  onSend,
  pending,
  error,
  placeholder,
  ariaLabel,
  autoFocus = false,
}: {
  value: string
  onChange: (value: string) => void
  onSend: () => Promise<void>
  pending: boolean
  error: string | null
  placeholder: string
  ariaLabel: string
  autoFocus?: boolean
}) {
  const areaRef = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const area = areaRef.current
    if (area === null) return
    area.style.height = '0px'
    area.style.height = `${String(Math.min(area.scrollHeight, 128))}px`
  }, [value])

  const trySend = (): void => {
    if (pending || value.trim() === '') return
    void onSend()
  }

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) return
    event.preventDefault()
    trySend()
  }

  return (
    <div className={css.composer}>
      <div className={css.composerBox} data-pending={pending || undefined}>
        <textarea
          ref={areaRef}
          rows={1}
          value={value}
          onChange={event => { onChange(event.target.value) }}
          onKeyDown={onKeyDown}
          placeholder={placeholder}
          aria-label={ariaLabel}
          // Keep the draft editable while pending; only repeat sends are blocked.
          autoFocus={autoFocus}
        />
        <button
          type="button"
          className={css.sendButton}
          aria-label="发送"
          title="发送"
          disabled={pending || value.trim() === ''}
          onClick={trySend}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="M12 19V5" />
            <path d="m5 12 7-7 7 7" />
          </svg>
        </button>
      </div>
      {error !== null
        ? <div className={css.composerError} role="alert">{error}</div>
        : <div className={css.composerHint}>Enter 发送 · Shift+Enter 换行</div>}
    </div>
  )
}
