import { useEffect, useRef } from 'react'
import type { NativeMessage, NativeTarget } from '../native.ts'
import css from './ChaosPanel.module.css'

/**
 * Sidecar Thread page: read the replies here. Sending stays on the official
 * composer; the dock says "回复 Thread".
 */
export function ThreadPanel({
  thread,
  parentName,
  messages,
  names,
  kinds,
}: {
  thread: NativeTarget
  parentName: string
  messages: readonly NativeMessage[]
  names: ReadonlyMap<string, string>
  kinds: ReadonlyMap<string, string>
  onClose?: () => void
}) {
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const node = scrollRef.current
    if (node !== null) node.scrollTop = node.scrollHeight
  }, [messages.length, thread.id])

  const rootSuffix = thread.rootMessageId?.slice(-5) ?? thread.id.slice(-5)

  return (
    <section className={css.threadPanel} aria-label={`Thread ${parentName} · ${rootSuffix}`}>
      <div ref={scrollRef} className={css.threadPanelMessages}>
        {messages.length === 0 && <p className={css.empty}>Thread 里还没有回复。</p>}
        {messages.map(message => (
          <div key={message.id} className={css.messageRow} data-has-header>
            <div className={css.messageRowHeader}>
              <strong>{names.get(message.authorId) ?? message.authorId}</strong>
              {kinds.get(message.authorId) === 'agent' && <span className={css.badge}>agent</span>}
              <time>{new Date(message.createdAtMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</time>
            </div>
            <p className={css.messageText}>{message.text}</p>
          </div>
        ))}
      </div>
    </section>
  )
}
