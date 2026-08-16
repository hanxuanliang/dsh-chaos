import { useEffect, useRef } from 'react'
import type { NativeMessage, NativeTarget } from '../native.ts'
import { Avatar } from './Avatar.tsx'
import { Composer } from './Composer.tsx'
import css from './ChaosPanel.module.css'

/**
 * Right-rail Thread panel: the main conversation stays put while the thread
 * is read and answered here. The composer is the same shared multiline
 * component as the main conversation and is pinned to the panel bottom.
 * Follow state is implicit (participating follows); there is no manual
 * follow switch in the panel.
 */
export function ThreadPanel({
  thread,
  parentName,
  messages,
  names,
  kinds,
  draft,
  onDraftChange,
  pending,
  error,
  onClose,
  onSend,
}: {
  thread: NativeTarget
  parentName: string
  messages: readonly NativeMessage[]
  names: ReadonlyMap<string, string>
  kinds: ReadonlyMap<string, string>
  draft: string
  onDraftChange: (value: string) => void
  pending: boolean
  error: string | null
  onClose: () => void
  onSend: () => Promise<void>
}) {
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const node = scrollRef.current
    if (node !== null) node.scrollTop = node.scrollHeight
  }, [messages.length, thread.id])

  const rootSuffix = thread.rootMessageId?.slice(-5) ?? thread.id.slice(-5)

  return (
    <section className={css.threadPanel} aria-label={`Thread ${parentName} · ${rootSuffix}`}>
      <header className={css.threadPanelHeader}>
        <div className={css.threadPanelTitle}>
          <span className={css.targetKind}>THREAD</span>
          <strong>{parentName} · {rootSuffix}</strong>
        </div>
        <button type="button" className={css.iconButton} aria-label="关闭 Thread 面板" onClick={onClose}>×</button>
      </header>
      <div ref={scrollRef} className={css.threadPanelMessages}>
        {messages.length === 0 && <p className={css.empty}>Thread 里还没有回复。</p>}
        {messages.map(message => (
          <div key={message.id} className={css.messageRow} data-has-header>
            <div className={css.messageRowHeader}>
              <Avatar seed={message.authorId} size={18} />
              <strong>{names.get(message.authorId) ?? message.authorId}</strong>
              {kinds.get(message.authorId) === 'agent' && <span className={css.badge}>agent</span>}
              <time>{new Date(message.createdAtMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
            </div>
            <p className={css.messageText}>{message.text}</p>
          </div>
        ))}
      </div>
      <Composer
        value={draft}
        onChange={onDraftChange}
        onSend={onSend}
        pending={pending}
        error={error}
        placeholder="回复 Thread"
        ariaLabel="回复 Thread"
      />
    </section>
  )
}
